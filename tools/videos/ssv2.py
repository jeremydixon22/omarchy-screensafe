"""Experimental indexed elementary-stream video. No codec in this module.

Only the fixed-profile encoder and the isolated decoder parse codec syntax.
The prototype keeps 16-bit frame references and a bounded 720p24 window.
"""
import importlib.util
import math
import os
from pathlib import Path
import stat
import struct
import tempfile
import time

MAGIC = b'SSVID002'
HEADER = struct.Struct('<8s14I')
ENTRY = struct.Struct('<QII')
FPS, GOP, MAX_FRAMES = 24, 24, 65535
MAX_SECONDS = MAX_FRAMES / FPS
MAX_FRAME = 2 << 20
MAX_ARCHIVE = 512 << 20
H264, AV1 = 2, 1
ENCODE_SECONDS = 30
AV1_ENCODE_SECONDS = 4
IMPORT_DEADLINE = 1800

spec = importlib.util.spec_from_file_location('screensafe_video_import', Path(__file__).with_name('import.py'))
media = importlib.util.module_from_spec(spec)
spec.loader.exec_module(media)


def read_index(fd):
    meta = os.fstat(fd)
    if not stat.S_ISREG(meta.st_mode) or not HEADER.size <= meta.st_size <= MAX_ARCHIVE:
        raise ValueError('invalid compressed video file size/type')
    header = os.pread(fd, HEADER.size, 0)
    if len(header) != HEADER.size:
        raise ValueError('truncated compressed video header')
    magic, w, h, fps, count, codec, gop, flags, config, *reserved = HEADER.unpack(header)
    if (magic != MAGIC or not 2 <= w <= 1280 or not 2 <= h <= 720 or w % 2 or h % 2
            or fps != FPS or not 1 <= count <= MAX_FRAMES or codec not in (H264, AV1)
            or gop != GOP or flags != 1 or config or any(reserved)):
        raise ValueError('unsupported compressed video profile')
    size = count * ENTRY.size
    raw = os.pread(fd, size, HEADER.size)
    if len(raw) != size:
        raise ValueError('truncated compressed video index')
    entries, end = [], HEADER.size + size
    for i, (offset, length, mark) in enumerate(ENTRY.iter_unpack(raw)):
        if offset != end or not 1 <= length <= MAX_FRAME or mark != int(i % GOP == 0):
            raise ValueError('invalid compressed video frame index')
        end = offset + length
        if end > meta.st_size:
            raise ValueError('compressed frame extends beyond file')
        entries.append((offset, length))
    if end != meta.st_size:
        raise ValueError('unexpected compressed video trailing data')
    return dict(width=w, height=h, fps=fps, frames=count, codec=codec, gop=gop, entries=entries)


def annexb_frames(chunks):
    """Split bounded Annex B NALs at encoder-required AUDs, never parse slices."""
    pending, frame = bytearray(), bytearray()
    started = False
    def nal(data):
        nonlocal frame, started
        if not data or len(data) > MAX_FRAME:
            raise ValueError('invalid H.264 unit size')
        kind = data[0] & 31
        if kind == 9:
            previous = bytes(frame) if started else None
            frame = bytearray(b'\0\0\0\1' + data)
            started = True
            return previous
        if not started or kind not in (1, 5, 6, 7, 8, 12):
            raise ValueError('unexpected normalized H.264 unit')
        frame.extend(b'\0\0\0\1' + data)
        if len(frame) > MAX_FRAME:
            raise ValueError('compressed frame exceeds its limit')
        return None
    for chunk in chunks:
        pending.extend(chunk)
        while True:
            first = pending.find(b'\0\0\1')
            if first < 0:
                if len(pending) > 4:
                    raise ValueError('missing Annex B prefix')
                break
            if any(pending[:first]):
                raise ValueError('unexpected Annex B prefix data')
            following = pending.find(b'\0\0\1', first + 3)
            if following < 0:
                if len(pending) > MAX_FRAME + 8:
                    raise ValueError('oversized H.264 unit')
                break
            data = bytes(pending[first+3:following]).rstrip(b'\0')
            del pending[:following]
            previous = nal(data)
            if previous is not None:
                yield previous
    first = pending.find(b'\0\0\1')
    if first < 0:
        raise ValueError('incomplete Annex B stream')
    previous = nal(bytes(pending[first+3:]).rstrip(b'\0'))
    if previous is not None:
        yield previous
    if not started or not frame:
        raise ValueError('empty H.264 stream')
    yield bytes(frame)


def obu_frames(chunks):
    """Bounded OBU framing only: temporal delimiters and length-delimited units."""
    pending, frame = bytearray(), bytearray()
    started = False
    for chunk in chunks:
        pending.extend(chunk)
        while pending:
            header = pending[0]
            if header & 0x81 or not header & 2:
                raise ValueError('invalid normalized AV1 OBU header')
            kind = (header >> 3) & 15
            at = 2 if header & 4 else 1
            if len(pending) <= at:
                break
            length, shift = 0, 0
            for i in range(8):
                if at + i >= len(pending):
                    break
                byte = pending[at+i]
                length |= (byte & 127) << shift
                shift += 7
                if not byte & 128:
                    prefix = at + i + 1
                    break
            else:
                raise ValueError('oversized AV1 OBU length')
            if at + i >= len(pending):
                break
            if length > MAX_FRAME:
                raise ValueError('oversized AV1 OBU')
            total = prefix + length
            if len(pending) < total:
                break
            unit = bytes(pending[:total]);del pending[:total]
            if kind == 2:
                if length:
                    raise ValueError('invalid AV1 temporal delimiter')
                if started:
                    yield bytes(frame)
                frame, started = bytearray(), True
            if not started:
                raise ValueError('AV1 stream lacks temporal delimiter')
            frame.extend(unit)
            if len(frame) > MAX_FRAME:
                raise ValueError('oversized AV1 temporal unit')
        if len(pending) > MAX_FRAME + 16:
            raise ValueError('oversized AV1 pending unit')
    if pending or not started or not frame:
        raise ValueError('incomplete AV1 stream')
    yield bytes(frame)


def check_profile(fd, offset, count, width, height, codec, seconds=90):
    """Confined full decode of one bounded job validates actual frame metadata."""
    form = 'h264' if codec == H264 else 'obu'
    command = ['/usr/bin/ffprobe','-v','error','-skip_initial_bytes',str(offset),'-f',form,
        '-select_streams','v:0','-show_frames','-show_entries','frame=key_frame,width,height,pix_fmt,pict_type',
        '-of','compact=p=0:nk=0','/input.mp4']
    pending, seen, total = bytearray(), 0, 0
    for chunk in media.output_chunks(fd,command,seconds):
        pending.extend(chunk);total+=len(chunk)
        if total > 1 << 20:
            raise ValueError('compressed profile metadata exceeds its limit')
        while b'\n' in pending:
            line,_,pending = pending.partition(b'\n')
            if len(line)>4096:raise ValueError('oversized compressed frame metadata')
            if not line:continue
            fields=dict(part.split('=',1) for part in line.decode('ascii').split('|') if '=' in part)
            if (fields.get('key_frame')!=str(int(seen%GOP==0)) or fields.get('width')!=str(width)
                    or fields.get('height')!=str(height) or fields.get('pix_fmt')!='yuv420p'
                    or fields.get('pict_type') not in ('I','P')):
                raise ValueError('encoder did not produce the fixed independent-GOP profile')
            seen+=1
            if seen>count:raise ValueError('compressed stream emitted extra frames')
        if len(pending)>4096:raise ValueError('oversized incomplete frame metadata')
    if pending or seen!=count:
        raise ValueError('compressed profile frame count differs')


def import_video(source, destination, codec=H264):
    """Explicit normalization; codec tools remain in the existing CPU-only cage."""
    import json
    began=time.monotonic()
    def remaining():
        left=IMPORT_DEADLINE-(time.monotonic()-began)
        if left<=0:raise ValueError('compressed import total time budget exceeded')
        return left
    if codec not in (H264, AV1):
        raise ValueError('unsupported codec')
    if os.path.lexists(destination):
        raise FileExistsError('destination already exists')
    source_fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    payload_name = output_name = None
    try:
        before = os.fstat(source_fd)
        if not stat.S_ISREG(before.st_mode) or not 0 < before.st_size <= media.MAX_INPUT:
            raise ValueError('input must be a regular MP4 of at most 2 GiB')
        raw = bytearray()
        for data in media.output_chunks(source_fd, ['/usr/bin/ffprobe','-v','error','-f','mov','-protocol_whitelist','file',
                '-select_streams','v:0','-show_entries','format=duration:stream=width,height,duration,color_transfer','-of','json','/input.mp4'],min(15,remaining())):
            raw.extend(data)
            if len(raw) > 65536:
                raise ValueError('oversized metadata')
        info = json.loads(raw);stream = info['streams'][0];duration = float(stream.get('duration',info['format']['duration']))
        if (not math.isfinite(duration) or not 0 < duration <= MAX_SECONDS
                or not all(isinstance(stream.get(k),int) and stream[k]>=2 for k in ['width','height'])
                or stream['width']*stream['height']>64_000_000):
            raise ValueError('source exceeds the bounded 16-bit video profile')
        if stream.get('color_transfer') in ['smpte2084','arib-std-b67']:
            raise ValueError('the experimental video profile requires SDR input')
        scale = min(1, 1280/stream['width'], 720/stream['height'])
        width, height = max(2,int(stream['width']*scale)//2*2), max(2,int(stream['height']*scale)//2*2)
        destination = Path(destination);destination.parent.mkdir(parents=True,exist_ok=True)
        with tempfile.NamedTemporaryFile(prefix='.screensafe-ssv2-',dir=destination.parent,delete=False) as payload:
            payload_name = Path(payload.name)
            entries=[]
            job_seconds = ENCODE_SECONDS if codec == H264 else AV1_ENCODE_SECONDS
            for part in range(math.ceil(duration/job_seconds)):
                remaining()
                start=part*job_seconds;seconds=min(job_seconds,duration-start)
                command = ['/usr/bin/ffmpeg','-v','error','-nostdin','-threads','2','-filter_threads','1','-ss',str(start),'-f','mov',
                    '-protocol_whitelist','file','-i','/input.mp4','-t',str(seconds),'-map','0:v:0','-an','-sn','-dn','-map_metadata','-1',
                    '-vf',f'scale={width}:{height},setsar=1,fps={FPS}','-frames:v',str(min(job_seconds*FPS,MAX_FRAMES-len(entries)+1)),'-threads','2',
                    '-pix_fmt','yuv420p','-g',str(GOP),'-keyint_min',str(GOP)]
                if codec == H264:
                    command += ['-c:v','libx264','-preset','medium','-tune','fastdecode','-crf','16','-bf','0','-sc_threshold','0','-x264-params',
                        f'keyint={GOP}:min-keyint={GOP}:scenecut=0:bframes=0:repeat-headers=1:aud=1:open-gop=0','-f','h264','pipe:1']
                    splitter = annexb_frames
                else:
                    command += ['-c:v','libaom-av1','-usage','good','-cpu-used','6','-crf','24','-b:v','0',
                        '-lag-in-frames','0','-auto-alt-ref','0','-row-mt','1','-f','obu','pipe:1']
                    splitter = obu_frames
                offset=payload.tell();initial=len(entries)
                for frame in splitter(media.output_chunks(source_fd,command,min(90,remaining()))):
                    remaining()
                    if len(entries) >= MAX_FRAMES or payload.tell()+len(frame)+HEADER.size+(len(entries)+1)*ENTRY.size > MAX_ARCHIVE:
                        raise ValueError('compressed video exceeds frame/storage limit')
                    entries.append((payload.tell(),len(frame)));payload.write(frame)
                produced=len(entries)-initial
                if not produced or (seconds==job_seconds and produced!=job_seconds*FPS):
                    raise ValueError('compressed segment duration differs')
                payload.flush();check_profile(payload.fileno(),offset,produced,width,height,codec,min(90,remaining()))
            if not entries:
                raise ValueError('empty compressed stream')
            payload.flush()
        with tempfile.NamedTemporaryFile(prefix='.screensafe-ssv2-',dir=destination.parent,delete=False) as output:
            output_name = Path(output.name)
            output.write(HEADER.pack(MAGIC,width,height,FPS,len(entries),codec,GOP,1,0,*([0]*6)))
            start = HEADER.size + len(entries)*ENTRY.size
            for i,(offset,length) in enumerate(entries):
                output.write(ENTRY.pack(start+offset,length,int(i%GOP==0)))
            with payload_name.open('rb') as payload:
                while block:=payload.read(1<<20):
                    remaining();output.write(block)
            output.flush();os.fsync(output.fileno());read_index(output.fileno())
        after = os.fstat(source_fd)
        remaining()
        if (before.st_size,before.st_mtime_ns,before.st_ctime_ns)!=(after.st_size,after.st_mtime_ns,after.st_ctime_ns):
            raise ValueError('source changed during conversion')
        os.link(output_name,destination)
        return dict(path=str(destination),width=width,height=height,fps=FPS,frames=len(entries),bytes=destination.stat().st_size,codec=codec)
    finally:
        os.close(source_fd)
        for p in [payload_name,output_name]:
            if p is not None:p.unlink(missing_ok=True)


if __name__ == '__main__':
    import argparse
    import signal
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('source',type=Path);parser.add_argument('destination',type=Path)
    parser.add_argument('--codec',choices=['h264','av1'],default='h264')
    args=parser.parse_args()
    def interrupted(*unused):raise InterruptedError('compressed import cancelled')
    signal.signal(signal.SIGTERM,interrupted)
    with media.ParentLifetime():
        print(import_video(args.source,args.destination,H264 if args.codec=='h264' else AV1))
