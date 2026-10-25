import importlib.util
import json
import os
from pathlib import Path
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch

spec = importlib.util.spec_from_file_location('video_import', Path(__file__).with_name('import.py'))
media = importlib.util.module_from_spec(spec)
spec.loader.exec_module(media)

class ImportTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.source = self.root/'source.mp4'
        subprocess.run(['/usr/bin/ffmpeg', '-v', 'error', '-f', 'lavfi', '-i', 'color=c=blue:s=64x48:d=0.25:r=24',
                        '-an', '-c:v', 'mpeg4', '-threads', '1', str(self.source)], check=True)
    def tearDown(self):
        self.temp.cleanup()
    def test_real_confined_import_and_no_upscale(self):
        out = self.root/'clip.ssv'
        result = media.import_video(self.source, out)
        self.assertEqual((result['width'],result['height'],result['fps'],result['frames']), (64,48,24,6))
        data=out.read_bytes()
        header=media.HEADER.unpack_from(data)
        self.assertEqual(header[:5],(media.MAGIC,64,48,24,6))
        previous=None
        for i in range(6):
            at,length=media.ENTRY.unpack_from(data,media.HEADER.size+i*media.ENTRY.size)
            if previous is not None:self.assertEqual(at,previous)
            self.assertEqual(media.jpeg_size(data[at:at+length]),(64,48))
            previous=at+length
        self.assertEqual(previous,len(data))
        with self.assertRaises(FileExistsError):media.import_video(self.source,out)
        self.assertEqual(out.read_bytes(),data)
    def test_refuses_symlink_input_and_malformed_media(self):
        link=self.root/'link.mp4';link.symlink_to(self.source)
        with self.assertRaises(OSError):media.import_video(link,self.root/'symlink.ssv')
        fake=self.root/'fake.mp4';fake.write_text('#EXTM3U\nhttp://127.0.0.1/not-a-video\n')
        with self.assertRaises(ValueError):media.import_video(fake,self.root/'bad.ssv')
        self.assertFalse((self.root/'bad.ssv').exists())
    def test_bad_decoder_output_never_publishes_a_partial_archive(self):
        def fake_output(fd,command,seconds):
            if command[0].endswith('ffprobe'):
                yield json.dumps({'streams':[{'width':64,'height':48}],'format':{'duration':'1'}}).encode()
            else:yield b'\xff\xd8not-a-valid-jpeg\xff\xd9'
        with patch.object(media,'output_chunks',fake_output):
            with self.assertRaises(ValueError):media.import_video(self.source,self.root/'partial.ssv')
        self.assertFalse((self.root/'partial.ssv').exists())
        self.assertEqual(list(self.root.glob('.screensafe-video-*')),[])
    def test_import_worker_has_separate_network_namespace_and_no_home(self):
        fd=os.open(self.source,os.O_RDONLY)
        try:
            code="import os,json; print(json.dumps([os.path.exists('/home'),os.readlink('/proc/self/ns/net'),os.path.exists('/input.mp4')]))"
            raw=b''.join(media.output_chunks(fd,['/usr/bin/python3','-c',code],10))
        finally:os.close(fd)
        home,network,source=json.loads(raw)
        self.assertFalse(home)
        self.assertNotEqual(network,os.readlink('/proc/self/ns/net'))
        self.assertTrue(source)

if __name__=='__main__':unittest.main()
