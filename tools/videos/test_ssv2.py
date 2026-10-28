import fcntl
import hashlib
import os
import json
import platform
import signal
from pathlib import Path
import struct
import subprocess
import tempfile
import time
import unittest
from unittest.mock import patch

import ssv2
import play_v2


class CompressedVideoTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.temp=tempfile.TemporaryDirectory();cls.root=Path(cls.temp.name)
        cls.source=cls.root/'source.mp4'
        subprocess.run(['/usr/bin/ffmpeg','-v','error','-f','lavfi','-i','testsrc2=s=160x90:r=24:d=3',
            '-c:v','libx264','-threads','1',str(cls.source)],check=True)
        cls.clips={}
        for codec in [ssv2.H264,ssv2.AV1]:
            p=cls.root/f'{codec}.ssv';ssv2.import_video(cls.source,p,codec);cls.clips[codec]=p

    @classmethod
    def tearDownClass(cls):cls.temp.cleanup()

    def test_sequential_seek_film_neighbors_and_loop_use_immutable_snapshots(self):
        for codec,path in self.clips.items():
            fd=os.open(path,os.O_RDONLY);frames=play_v2.Frames()
            try:
                hashes={}
                for i in range(72):
                    pixels=frames.frame(fd,i)
                    self.assertEqual(fcntl.fcntl(pixels,fcntl.F_GET_SEALS),play_v2.SEALS)
                    hashes[i]=hashlib.sha256(os.pread(pixels,160*90*4,0)).digest()
                    self.assertLessEqual(len(frames.cache),play_v2.CACHE_FRAMES)
                # Deliberately switch GOPs, seek backwards, and straddle a loop.
                for base in [0,24,71,23,48,47,25,1,70]:
                    for offset in range(-4,5):
                        i=(base+offset)%72;snapshot=frames.frame(fd,i)
                        self.assertEqual(hashlib.sha256(os.pread(snapshot,160*90*4,0)).digest(),hashes[i],(codec,i))
                with self.assertRaises(OSError):os.pwrite(snapshot,b'bad',0)
                self.assertGreater(frames.hits,0)
                self.assertLess(frames.starts,40)
            finally:frames.close();os.close(fd)

    def test_input_must_be_read_only_and_retains_its_descriptor(self):
        with tempfile.TemporaryDirectory() as directory:
            p=Path(directory)/'clip.ssv';p.write_bytes(self.clips[ssv2.H264].read_bytes())
            writable=os.open(p,os.O_RDWR);frames=play_v2.Frames()
            try:
                with self.assertRaises(ValueError):frames.frame(writable,0)
            finally:os.close(writable)
            fd=os.open(p,os.O_RDONLY)
            try:
                a=os.pread(frames.frame(fd,0),160*90*4,0)
                p.rename(p.with_suffix('.old'));p.write_bytes(b'changed pathname')
                self.assertEqual(os.pread(frames.frame(fd,0),160*90*4,0),a)
            finally:frames.close();os.close(fd)

    def test_direct_delivery_matches_snapshot_pixels_across_seeks_and_seams(self):
        for path in self.clips.values():
            fd=os.open(path,os.O_RDONLY);reference=play_v2.Frames();direct=play_v2.StreamFrames()
            try:
                for n in [0,1,24,25,23,71,0,1,70,71,2,23,24,47,48]:
                    expected=os.pread(reference.frame(fd,n),160*90*4,0)
                    delivered=direct.frame(fd,n)
                    if direct.delivery==0:actual=os.pread(delivered,len(expected),0)
                    else:
                        actual=bytearray()
                        while len(actual)<len(expected):
                            data=os.read(delivered,len(expected)-len(actual))
                            self.assertTrue(data);actual.extend(data)
                    self.assertEqual(actual,expected,n)
            finally:reference.close();direct.close();os.close(fd)

    def test_header_index_and_trailing_data_fail_closed(self):
        valid=self.clips[ssv2.H264].read_bytes()
        with tempfile.TemporaryFile() as output:
            for at,value in [(20,0),(20,65536),(24,3),(28,0),(32,0),(36,1),(40,1),
                    (64,0),(72,0),(76,0),(64+24*16+12,0)]:
                bad=bytearray(valid);struct.pack_into('<I',bad,at,value)
                output.seek(0);output.truncate();output.write(bad);output.flush()
                with self.assertRaises(ValueError,msg=str(at)):ssv2.read_index(output.fileno())
            output.seek(0);output.truncate();output.write(valid+b'extra');output.flush()
            with self.assertRaises(ValueError):ssv2.read_index(output.fileno())

    def test_cpu_budget_recycling_preserves_the_next_frame(self):
        # Long clips must outlive a worker's cumulative CPU allowance. Inject
        # the accounting threshold rather than burning 100 CPU seconds here.
        for path in self.clips.values():
            for delivery in [play_v2.Frames,play_v2.StreamFrames]:
                fd=os.open(path,os.O_RDONLY);reference=play_v2.Frames();frames=delivery()
                def pixels(index):
                    delivered=frames.frame(fd,index)
                    if frames.delivery==0:return os.pread(delivered,160*90*4,0)
                    import socket
                    with socket.socket(fileno=os.dup(delivered)) as stream:
                        stream.settimeout(2);data=bytearray()
                        while len(data)<160*90*4:
                            block=stream.recv(160*90*4-len(data))
                            self.assertTrue(block);data.extend(block)
                    return data
                try:
                    pixels(0);previous=frames.decoder
                    with patch.object(previous,'cpu_seconds',return_value=101):
                        self.assertEqual(pixels(1),os.pread(reference.frame(fd,1),160*90*4,0))
                    self.assertEqual(frames.starts,2)
                    self.assertIsNotNone(previous.proc.poll())
                    self.assertEqual(pixels(2),os.pread(reference.frame(fd,2),160*90*4,0))
                finally:frames.close();reference.close();os.close(fd)

    def test_framing_is_independent_of_pipe_chunk_boundaries(self):
        for codec,path in self.clips.items():
            fd=os.open(path,os.O_RDONLY)
            try:
                info=ssv2.read_index(fd)
                expected=[os.pread(fd,n,at) for at,n in info['entries']]
            finally:os.close(fd)
            data=b''.join(expected)
            split=ssv2.annexb_frames if codec==ssv2.H264 else ssv2.obu_frames
            self.assertEqual(list(split([data])),expected)
            self.assertEqual(list(split(data[i:i+1] for i in range(len(data)))),expected)
        with self.assertRaises(ValueError):list(ssv2.obu_frames([b'\x12'+b'\xff'*9]))
        with self.assertRaises(ValueError):list(ssv2.annexb_frames([b'not h264']))

    def test_bad_encoding_cleans_temporary_files_and_never_overwrites(self):
        with tempfile.TemporaryDirectory() as directory:
            p=Path(directory)/'out.ssv';p.write_bytes(b'keep')
            with self.assertRaises(FileExistsError):ssv2.import_video(self.source,p)
            self.assertEqual(p.read_bytes(),b'keep');p.unlink()
            original=ssv2.media.output_chunks
            def broken(fd,command,seconds):
                if command[0].endswith('ffprobe'):yield from original(fd,command,seconds)
                else:yield b'invalid encoded stream'
            with patch.object(ssv2.media,'output_chunks',broken):
                with self.assertRaises(ValueError):ssv2.import_video(self.source,p)
            self.assertEqual(list(Path(directory).iterdir()),[])
            with patch.object(ssv2,'check_profile',side_effect=ValueError('bad GOP profile')):
                with self.assertRaises(ValueError):ssv2.import_video(self.source,p)
            self.assertEqual(list(Path(directory).iterdir()),[])

    def test_more_than_five_minutes_uses_bounded_jobs_and_16_bit_frames(self):
        with tempfile.TemporaryDirectory() as directory:
            source=Path(directory)/'long.mp4';destination=Path(directory)/'long.ssv'
            subprocess.run(['/usr/bin/ffmpeg','-v','error','-f','lavfi','-i','color=c=navy:s=16x16:r=24:d=306',
                '-c:v','libx264','-preset','ultrafast','-threads','1',str(source)],check=True)
            result=ssv2.import_video(source,destination)
            self.assertEqual(result['frames'],306*24)
            fd=os.open(destination,os.O_RDONLY);frames=play_v2.Frames()
            try:
                a=os.pread(frames.frame(fd,0),16*16*4,0)
                self.assertEqual(os.pread(frames.frame(fd,306*24-1),16*16*4,0),a)
            finally:frames.close();os.close(fd)

    def test_encoder_job_and_gop_boundaries_do_not_repeat_or_skip_frames(self):
        # Offscreen only: widely spaced greys make a one-frame phase error
        # unambiguous after lossy encoding. Check both 4s and 30s job boundaries.
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);source=root/'phases.mp4'
            subprocess.run(['/usr/bin/ffmpeg','-v','error','-f','lavfi','-i',
                "nullsrc=s=16x16:r=24:d=31,geq=lum='16+30*mod(N,8)':cb=128:cr=128",'-c:v','libx264','-crf','0','-threads','1',str(source)],check=True)
            for codec in [ssv2.H264,ssv2.AV1]:
                p=root/f'{codec}.ssv';ssv2.import_video(source,p,codec)
                fd=os.open(p,os.O_RDONLY);frames=play_v2.Frames()
                try:
                    for i in [0,22,23,24,95,96,97,719,720,721,743]:
                        data=os.pread(frames.frame(fd,i),16*16*4,0)
                        expected=round((i%8)*30*255/219)
                        self.assertLessEqual(abs(data[0]-expected),3,(codec,i,data[0],expected))
                finally:frames.close();os.close(fd)

    def test_parent_death_stops_broker_renderer_and_codec(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);ready=root/'ready';pidfile=root/'pid'
            fake=root/'renderer'
            fake.write_text('''#!/usr/bin/python3
import socket,struct,array,os,sys,time
from pathlib import Path
s=socket.socket(fileno=0);fd=os.open(sys.argv[2],os.O_RDONLY)
s.sendmsg([struct.pack('<4I',0x32565353,1,0,0)],[(socket.SOL_SOCKET,socket.SCM_RIGHTS,array.array('i',[fd]))])
s.recvmsg(32,256);Path(sys.argv[3]).write_text('ready');time.sleep(60)
''');fake.chmod(0o755)
            wrapper=Path(play_v2.__file__).resolve()
            driver='''import subprocess,sys,time
from pathlib import Path
p=subprocess.Popen([sys.executable,sys.argv[1],'--renderer',sys.argv[2],'--','check',sys.argv[3],sys.argv[4]])
Path(sys.argv[5]).write_text(str(p.pid));time.sleep(60)
'''
            tracked=set()
            def alive(pid):
                try:return Path(f'/proc/{pid}/stat').read_text().split(') ',1)[1].split()[0]!='Z'
                except OSError:return False
            with tempfile.TemporaryFile() as log:
                parent=subprocess.Popen(['/usr/bin/python3','-c',driver,str(wrapper),str(fake),str(self.clips[ssv2.H264]),str(ready),str(pidfile)],stdout=log,stderr=log)
                try:
                    deadline=time.monotonic()+5
                    while not ready.exists() and time.monotonic()<deadline:time.sleep(.02)
                    self.assertTrue(ready.exists())
                    tracked={int(pidfile.read_text())}
                    for _ in range(6):
                        for p in Path('/proc').glob('[0-9]*/stat'):
                            try:
                                if int(p.read_text().split(') ',1)[1].split()[1]) in tracked:tracked.add(int(p.parent.name))
                            except (OSError,ValueError):pass
                    self.assertGreaterEqual(len(tracked),3)
                    parent.kill();parent.wait(timeout=2)
                    deadline=time.monotonic()+5
                    while any(alive(pid) for pid in tracked) and time.monotonic()<deadline:time.sleep(.02)
                    self.assertFalse(any(alive(pid) for pid in tracked),'orphaned prototype process')
                finally:
                    if parent.poll() is None:parent.kill();parent.wait(timeout=2)
                    for pid in tracked:
                        if alive(pid):os.kill(pid,signal.SIGKILL)

    def test_actual_renderer_composites_each_style_and_checks_decoded_strobes(self):
        repo=Path(__file__).resolve().parents[2]
        renderer=Path(os.environ.get('SCREENSAFE_TEST_RENDERER',repo/'bin'/('screensafe-'+platform.machine())))
        wrapper=Path(play_v2.__file__).resolve()
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);clips=root/'clips';clips.mkdir()
            (clips/'clip.ssv').write_bytes(self.clips[ssv2.H264].read_bytes())
            for style in [0,1,2,3,4,5]:
                output=root/f'{style}.png'
                command=['/usr/bin/python3',str(wrapper),'--renderer',str(renderer),'--','render',str(repo/'packs/videos'),
                    '--videos',str(clips),'--size','320x180','--time','7.2' if style==4 else '1.2',
                    '--settings',f'style={style},each=10,order=1','--out',str(output)]
                result=subprocess.run(command,capture_output=True,text=True,timeout=15)
                self.assertEqual(result.returncode,0,result.stderr)
                self.assertTrue(output.read_bytes().startswith(b'\x89PNG'))
                stats=next(json.loads(line)['ssv2'] for line in result.stderr.splitlines() if line.startswith('{"ssv2"'))
                self.assertGreater(stats['requests'],0);self.assertEqual(stats['failures'],0)
            source=root/'strobe.mp4'
            subprocess.run(['/usr/bin/ffmpeg','-v','error','-f','lavfi','-i',
                "nullsrc=s=64x64:r=24:d=2,geq=lum='255*mod(N,2)':cb=128:cr=128",'-c:v','libx264','-threads','1',str(source)],check=True)
            (clips/'clip.ssv').unlink();ssv2.import_video(source,clips/'strobe.ssv')
            pack=root/'pack';pack.mkdir();(pack/'screensafe.toml').write_text('format=2\nname="Offscreen guard probe"\nfps=24\nstage="screen"\nfade=0\n[[scene]]\nduration=2\n[[scene.draw]]\nkind="image"\nsource="videos"\nposition="time"\nw=1\nh=1\n')
            result=subprocess.run(['/usr/bin/python3',str(wrapper),'--renderer',str(renderer),'--','check',str(pack),
                '--videos',str(clips),'--size','192x108','--seconds','2'],capture_output=True,text=True,timeout=15)
            report=json.loads(result.stdout)
            self.assertFalse(report['flash']['passes'])
            self.assertGreater(report['flash']['mostFlashingCells'],0)
            self.assertEqual(report['warnings'],[])


if __name__=='__main__':unittest.main()
