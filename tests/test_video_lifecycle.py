"""Actual broker entry point: early disconnect, exit codes and bounded cleanup."""
import contextlib
import array
import io
import os
from pathlib import Path
import selectors
import socket
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT/'tools/videos'))
import play_v2


class VideoLifecycleTests(unittest.TestCase):
    def broker(self, body):
        with tempfile.TemporaryDirectory(prefix='screensafe-disconnect-') as directory:
            renderer = Path(directory)/'renderer'
            renderer.write_text('#!/usr/bin/python3\nimport os,signal,socket,time,sys\nsignal.alarm(4)\n'+body)
            renderer.chmod(0o700)
            command = [sys.executable, '-B', str(ROOT/'tools/videos/play_v2.py'), '--renderer', str(renderer), '--', 'run']
            t = time.monotonic()
            result = subprocess.run(command, stdin=subprocess.DEVNULL, capture_output=True, text=True, timeout=6)
            return result, time.monotonic()-t

    def test_disconnect_of_still_running_renderer_is_explicit_and_bounded(self):
        p, seconds = self.broker("print('screensafe: video frame 17 failed: original pixel failure',file=sys.stderr,flush=True)\ns=socket.socket(fileno=0);s.shutdown(socket.SHUT_RDWR);s.close()\ntime.sleep(60)\n")
        self.assertEqual(p.returncode, 3, p.stderr)
        self.assertLess(seconds, 3)
        self.assertIn('original pixel failure', p.stderr)
        self.assertIn('decoder channel disconnected before renderer exit', p.stderr)
        self.assertNotIn('TimeoutExpired', p.stderr)
        self.assertNotIn('Traceback', p.stderr)

    def test_unresponsive_renderer_is_killed_within_cleanup_budget(self):
        p, seconds = self.broker("signal.signal(signal.SIGTERM,signal.SIG_IGN)\ns=socket.socket(fileno=0);s.shutdown(socket.SHUT_RDWR);s.close()\ntime.sleep(60)\n")
        self.assertEqual(p.returncode, 3, p.stderr)
        self.assertLess(seconds, 3.5)
        self.assertNotIn('TimeoutExpired', p.stderr)

    def test_normal_exit_and_actual_pack_failure_codes_are_preserved(self):
        for code in [0,3]:
            with self.subTest(code=code):
                p, seconds = self.broker(f'sys.exit({code})\n')
                self.assertEqual(p.returncode, code, p.stderr)
                self.assertLess(seconds, 1)
                self.assertNotIn('disconnected before renderer exit', p.stderr)

    def test_software_launch_clears_inherited_hardware_recovery_authority(self):
        with patch.dict(os.environ,{'SCREENSAFE_SSV2_RECOVERY':'1'}):
            p,_=self.broker("print(os.environ.get('SCREENSAFE_SSV2_RECOVERY','absent'),flush=True)\n")
        self.assertEqual(p.returncode,0,p.stderr)
        self.assertEqual(p.stdout.strip(),'absent')

    def test_direct_decoder_stderr_and_natural_exit_status_survive_cleanup(self):
        decoder = play_v2.Decoder.__new__(play_v2.Decoder)
        decoder.proc = subprocess.Popen([sys.executable,'-c',"import sys;print('driver decode fault',file=sys.stderr);sys.exit(7)"],
                                        stdin=subprocess.DEVNULL, stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
        decoder.proc.wait(timeout=2)
        decoder.output = io.BytesIO()
        decoder.selector = selectors.DefaultSelector()
        decoder.errors = bytearray()
        message = io.StringIO()
        with contextlib.redirect_stderr(message):
            decoder.close()
        self.assertIn('decoder exited with status 7', message.getvalue())
        self.assertIn('driver decode fault', message.getvalue())
        self.assertTrue(decoder.proc.stderr.closed)
        decoder.close()  # A failed replacement must not lose its cause in double cleanup.

    def test_renderer_closing_during_reply_does_not_replace_its_exit_with_broken_pipe(self):
        for refusal in [False,True]:
            with self.subTest(refusal=refusal):
                fd=os.open('/dev/null',os.O_RDONLY)
                class Channel:
                    def recvmsg(self,*args):
                        return play_v2.REQUEST.pack(play_v2.MAGIC,1,0,0),[(socket.SOL_SOCKET,socket.SCM_RIGHTS,array.array('i',[fd]).tobytes())],0,None
                    def send(self,*args):raise BrokenPipeError('peer exited during refusal')
                    def sendmsg(self,*args):raise BrokenPipeError('peer exited during pixels')
                class Frames:
                    starts=decodes=hits=delivery=0
                    info={'width':2,'height':2}
                    def __init__(self,*unused):pass
                    def frame(self,*unused):
                        if refusal:raise ValueError('original codec failure')
                        return fd
                    def close(self):pass
                with patch.object(play_v2,'StreamFrames',Frames),contextlib.redirect_stderr(io.StringIO()) as message:
                    play_v2.serve(Channel(),None)
                self.assertNotIn('Broken pipe',message.getvalue())
                if refusal:self.assertIn('original codec failure',message.getvalue())
                with self.assertRaises(OSError):os.fstat(fd)


if __name__ == '__main__':
    unittest.main()
