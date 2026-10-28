#!/usr/bin/env python3
"""Local SSVv2 prototype broker: isolated CPU decode, sealed pixel snapshots.

Run: play_v2.py --renderer /absolute/screensafe -- run --pack ... --videos ...
This does not install, change preferences or enable the offline-approval player.
"""
import argparse
import array
from collections import OrderedDict
import fcntl
import json
import os
from pathlib import Path
import selectors
import signal
import socket
import struct
import subprocess
import time

import ssv2

MAGIC = 0x32565353
REQUEST = struct.Struct('<4I')
RESPONSE = struct.Struct('<8I')
SEALS = fcntl.F_SEAL_SEAL | fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK
CACHE_FRAMES = 12
FRAME_DEADLINE = 5


def stamp(fd):
    s = os.fstat(fd)
    return (s.st_dev,s.st_ino,s.st_size,s.st_mtime_ns,s.st_ctime_ns)


class Decoder:
    """One bounded subprocess, lazily consumed through pipe backpressure."""
    def __init__(self, fd, info, start):
        self.next = start
        codec = 'h264' if info['codec'] == ssv2.H264 else 'libdav1d'
        form = 'h264' if info['codec'] == ssv2.H264 else 'obu'
        offset = info['entries'][start][0]
        command = ['/usr/bin/ffmpeg','-v','error','-xerror','-err_detect','explode','-nostdin','-threads','2','-filter_threads','1',
            '-hwaccel','none','-skip_initial_bytes',str(offset),'-probesize','32','-analyzeduration','1','-fpsprobesize','0',
            '-f',form,'-c:v',codec,'-r',str(info['fps']),'-protocol_whitelist','file','-i','/input.mp4',
            '-map','0:v:0','-an','-sn','-dn','-threads','2','-vf',f"scale={info['width']}:{info['height']},format=rgba",
            '-frames:v',str(info['frames']-start),'-fps_mode','passthrough','-f','rawvideo','-flush_packets','1','pipe:1']
        with ssv2.media.media_process_filter() as policy:
            self.proc = subprocess.Popen([ssv2.media.media_launcher()]+ssv2.media.sandbox(fd,command,policy),
                pass_fds=(fd,policy),stdin=subprocess.DEVNULL,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
        self.selector=selectors.DefaultSelector()
        self.selector.register(self.proc.stdout,selectors.EVENT_READ,'out')
        self.selector.register(self.proc.stderr,selectors.EVENT_READ,'err')
        self.pending=bytearray();self.errors=bytearray()
        self.frame_bytes=info['width']*info['height']*4

    def cpu_seconds(self):
        # Bwrap may have an outer monitor plus a PID-namespace init. Count only
        # its small descendant tree; this never accepts paths from the client.
        ticks=os.sysconf('SC_CLK_TCK');todo=[self.proc.pid];total=0
        for _ in range(8):
            if not todo:break
            pid=todo.pop()
            try:
                p=Path('/proc')/str(pid)
                fields=(p/'stat').read_text().split(') ',1)[1].split()
                total+=int(fields[11])+int(fields[12])
                todo.extend(int(x) for x in (p/'task'/str(pid)/'children').read_text().split())
            except (OSError,ValueError):pass
        return total/ticks

    def read(self, deadline):
        while len(self.pending)<self.frame_bytes:
            if time.monotonic()>=deadline:
                raise ValueError('compressed frame deadline exceeded')
            if not self.selector.get_map():
                raise ValueError('compressed decoder ended early: '+self.errors.decode(errors='replace')[-500:])
            for key,_ in self.selector.select(min(.1,max(0,deadline-time.monotonic()))):
                data=os.read(key.fd,min(65536,self.frame_bytes-len(self.pending)) if key.data=='out' else 4096)
                if not data:self.selector.unregister(key.fileobj)
                elif key.data=='out':self.pending.extend(data)
                else:
                    self.errors.extend(data)
                    if len(self.errors)>65536:raise ValueError('compressed decoder error budget exceeded')
        data=bytes(self.pending);self.pending.clear();self.next+=1
        return data

    def close(self):
        self.selector.close()
        if self.proc.poll() is None:
            self.proc.terminate()
            try:self.proc.wait(timeout=1)
            except subprocess.TimeoutExpired:self.proc.kill();self.proc.wait(timeout=1)
        self.proc.stdout.close();self.proc.stderr.close()


class Frames:
    """One active clip, twelve immutable frames, no total-duration RAM growth."""
    def __init__(self):
        self.fd=None;self.info=None;self.identity=None;self.decoder=None
        self.cache=OrderedDict();self.starts=0;self.decodes=0;self.hits=0

    def close(self):
        if self.decoder is not None:self.decoder.close();self.decoder=None
        for fd in self.cache.values():os.close(fd)
        self.cache.clear()
        if self.fd is not None:os.close(self.fd);self.fd=None
        self.identity=None

    def frame(self, fd, target):
        if fcntl.fcntl(fd,fcntl.F_GETFL)&os.O_ACCMODE != os.O_RDONLY:
            raise ValueError('compressed input descriptor must be read-only')
        identity=stamp(fd)
        if identity!=self.identity:
            self.close();self.info=ssv2.read_index(fd)
            if stamp(fd)!=identity:raise ValueError('compressed video changed while opening')
            self.fd=os.dup(fd);self.identity=identity
        if not 0<=target<self.info['frames']:
            raise ValueError('compressed frame index out of range')
        if target in self.cache:
            self.hits+=1;self.cache.move_to_end(target)
            return self.cache[target]
        if (self.decoder is None or target<self.decoder.next or target-self.decoder.next>=ssv2.GOP
                or self.decoder.cpu_seconds()>100):
            if self.decoder is not None:self.decoder.close()
            self.decoder=Decoder(self.fd,self.info,target//ssv2.GOP*ssv2.GOP);self.starts+=1
        deadline=time.monotonic()+FRAME_DEADLINE
        # A cold/random request decodes no more than one fixed GOP. Sequential
        # playback reuses the same process and decodes one new frame per request.
        while self.decoder.next<=target:
            index=self.decoder.next;pixels=self.decoder.read(deadline);self.decodes+=1
            snapshot=os.memfd_create('screensafe-video-frame',os.MFD_CLOEXEC|os.MFD_ALLOW_SEALING)
            try:
                at=0
                while at<len(pixels):
                    written=os.write(snapshot,pixels[at:])
                    if written<=0:raise OSError('short pixel snapshot write')
                    at+=written
                fcntl.fcntl(snapshot,fcntl.F_ADD_SEALS,SEALS)
            except BaseException:os.close(snapshot);raise
            if index in self.cache:os.close(self.cache.pop(index))
            self.cache[index]=snapshot
            while len(self.cache)>CACHE_FRAMES:os.close(self.cache.popitem(last=False)[1])
        if stamp(self.fd)!=identity:raise ValueError('compressed video changed during decoding')
        return self.cache[target]


def serve(channel, renderer):
    frames=Frames();requests=0;failures=0
    try:
        while True:
            data,ancillary,flags,_=channel.recvmsg(REQUEST.size,socket.CMSG_SPACE(4*4),socket.MSG_CMSG_CLOEXEC)
            descriptors=[]
            try:
                unexpected=False
                for level,kind,raw in ancillary:
                    if level!=socket.SOL_SOCKET or kind!=socket.SCM_RIGHTS:
                        unexpected=True
                        continue
                    a=array.array('i');a.frombytes(raw[:len(raw)//a.itemsize*a.itemsize]);descriptors.extend(a)
                if not data:break
                if unexpected or flags&(socket.MSG_TRUNC|socket.MSG_CTRUNC) or len(data)!=REQUEST.size or len(descriptors)!=1:
                    raise ValueError('invalid video request')
                magic,sequence,target,reserved=REQUEST.unpack(data)
                if magic!=MAGIC or reserved or target>=ssv2.MAX_FRAMES:
                    raise ValueError('invalid bounded video request')
                requests+=1
                try:
                    fd=frames.frame(descriptors[0],target);info=frames.info
                    response=RESPONSE.pack(MAGIC,sequence,target,info['width'],info['height'],info['width']*info['height']*4,0,0)
                    sent=channel.sendmsg([response],[(socket.SOL_SOCKET,socket.SCM_RIGHTS,array.array('i',[fd]))])
                except InterruptedError:
                    raise
                except (ValueError,OSError) as error:
                    failures+=1;frames.close()
                    print('screensafe-video: '+str(error),file=__import__('sys').stderr,flush=True)
                    sent=channel.send(RESPONSE.pack(MAGIC,sequence,target,0,0,0,1,0))
                if sent!=RESPONSE.size:raise ValueError('short video response')
            finally:
                for fd in descriptors:os.close(fd)
    finally:
        stats={'requests':requests,'failures':failures,'decoderStarts':frames.starts,'decodedFrames':frames.decodes,'cacheHits':frames.hits}
        frames.close();print(json.dumps({'ssv2':stats}),file=__import__('sys').stderr,flush=True)


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--renderer',type=Path,required=True);parser.add_argument('args',nargs=argparse.REMAINDER)
    args=parser.parse_args();command=args.args[1:] if args.args[:1]==['--'] else args.args
    if not args.renderer.is_absolute() or not args.renderer.is_file() or not command:
        parser.error('an absolute renderer and its command are required')
    if command[0] not in ['run','check','render','thumbnail','catalog']:
        parser.error('this prototype wraps run/check/render/thumbnail/catalog; stdin-control commands use a different protocol')
    server,client=socket.socketpair(socket.AF_UNIX,socket.SOCK_SEQPACKET)
    # Peer EOF wakes recvmsg when the renderer exits. Avoid an idle polling
    # timer; bound only sends, so a non-reading client cannot strand the broker.
    server.setsockopt(socket.SOL_SOCKET,socket.SO_SNDTIMEO,struct.pack('@ll',2,0))
    def interrupted(*unused):raise InterruptedError('video broker cancelled')
    signal.signal(signal.SIGTERM,interrupted)
    child=None
    try:
        with ssv2.media.ParentLifetime():
            child=subprocess.Popen([str(args.renderer)]+command,stdin=client,env=dict(os.environ,SCREENSAFE_SSV2_WORKER='1'))
            client.close();serve(server,child);return child.wait(timeout=5)
    finally:
        client.close();server.close()
        if child is not None and child.poll() is None:
            child.terminate()
            try:child.wait(timeout=2)
            except subprocess.TimeoutExpired:child.kill();child.wait(timeout=2)


if __name__=='__main__':
    try:raise SystemExit(main())
    except (InterruptedError,KeyboardInterrupt):raise SystemExit(130)
