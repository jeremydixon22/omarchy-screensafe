"""Exercise the actual controller entry point, with harmless child substitutes."""
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch
from screensafe_runtime import ParentLifetime

ROOT = Path(__file__).resolve().parents[1]


def alive(pid):
    try:
        return Path(f'/proc/{pid}/stat').read_text().split(') ', 1)[1].split()[0] != 'Z'
    except FileNotFoundError:
        return False


def until(test, timeout=6):
    end = time.monotonic()+timeout
    while time.monotonic() < end:
        if test():
            return
        time.sleep(.03)
    raise AssertionError('bounded lifetime condition did not complete')


class LifetimeTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='screensafe-lifetime-')
        self.root = r = Path(self.temp.name)
        self.processes = []
        self.ids = []
        renderer = r/'renderer'
        renderer.write_text('#!/usr/bin/python3\nimport time\ntime.sleep(60)\n')
        renderer.chmod(0o700)
        (r/'controller.py').write_text('''from pathlib import Path
import sys,hashlib,json,time,os
sys.path.insert(0,sys.argv[1]);import manager as m
r=Path(sys.argv[2]);mode=sys.argv[3]
store=r/'store';store.mkdir(mode=0o700);key='a'*64;(store/(key+'.mp4')).write_bytes(b'x');(store/(key+'.json')).write_text('{}')
m.available=lambda a:True
m.plan=lambda a:{'eligible':True}
m.geometry=lambda:[16,16]
m.profile=lambda a,b:{'size':[16,16]}
m.lookup=lambda *a:(key,{'sha256':hashlib.sha256(b'x').hexdigest(),'bytes':1,'frames':1,'profile':{}},Path('/dev/null'),'software')
class FakeGPU:
 def __init__(self,path):self.path=path;self.pass_fds=()
 def __enter__(self):return self
 def __exit__(self,*unused):pass
m.RenderNode=FakeGPU;m.identity=lambda *a:{}
m.sandbox=lambda *a,**kw:['/bin/sleep','60']
def prepare(a):
 (r/'ready').touch()
 while not m.STOP:time.sleep(.03)
 raise ValueError('cancelled')
def hung(a):
 (r/'ready').touch()
 while True:
  try:time.sleep(60)
  except BaseException:
   if mode!='status-hung':raise
m.prepare=prepare;m.status=hung
sys.argv=['manager','status' if mode=='status-hung' else mode,'--renderer',str(r/'renderer'),'--worker',str(r/'unused'),'--root',str(store),'--battery-only','0']+sys.argv[4:]
sys.exit(m.cli())
''')
        self.driver = [sys.executable, str(r/'controller.py'), str(ROOT/'cache'), str(r)]

    def tearDown(self):
        for p in self.processes:
            if p.poll() is None:
                p.kill()
            p.wait(timeout=3)
        for pid in self.ids:
            if alive(pid):
                os.kill(pid, signal.SIGKILL)
        self.temp.cleanup()

    def children(self, pid):
        try:
            return [int(s) for s in Path(f'/proc/{pid}/task/{pid}/children').read_text().split()]
        except FileNotFoundError:
            return []

    def start(self, managed=False):
        p = subprocess.Popen(self.driver+['run']+(['--managed'] if managed else []),
                             stdin=subprocess.PIPE, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.processes.append(p)
        until(lambda: len(self.children(p.pid)) == 2)
        self.ids.extend(self.children(p.pid))
        return p

    def test_managed_control_eof_ends_controller_and_children(self):
        p = self.start(managed=True)
        p.stdin.close()
        self.assertEqual(p.wait(timeout=4), 0)
        until(lambda: not any(map(alive, self.ids)))

    def test_standalone_eof_keeps_running_but_term_cleans_up(self):
        p = self.start()
        p.stdin.close(); time.sleep(.15)
        self.assertIsNone(p.poll())
        p.terminate()
        self.assertEqual(p.wait(timeout=4), 143)
        until(lambda: not any(map(alive, self.ids)))

    def parent_death(self, action):
        parent = self.root/'parent.py'
        parent.write_text('''import json,subprocess,sys,time
from pathlib import Path
p=subprocess.Popen(json.loads(sys.argv[1]),stdin=subprocess.PIPE,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
Path(sys.argv[2]).write_text(str(p.pid));time.sleep(60)
''')
        p = subprocess.Popen([sys.executable, str(parent), json.dumps(self.driver+[action]), str(self.root/'pid')])
        self.processes.append(p)
        until(lambda: (self.root/'pid').exists())
        pid = int((self.root/'pid').read_text()); self.ids.append(pid)
        if action == 'run':
            until(lambda: len(self.children(pid)) == 2)
            self.ids.extend(self.children(pid))
        else:
            until(lambda: (self.root/'ready').exists())
        p.kill(); p.wait(timeout=3)
        until(lambda: not any(map(alive, self.ids)), timeout=5)

    def test_parent_kill_cleans_up_playback_tree(self):
        self.parent_death('run')

    def test_parent_kill_cancels_preparation(self):
        self.parent_death('prepare')

    def test_parent_kill_ends_even_an_unresponsive_status_helper(self):
        self.parent_death('status-hung')

    def test_status_deadline_signal_interrupts_a_short_command(self):
        p = subprocess.Popen(self.driver+['status'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        self.processes.append(p)
        until(lambda: (self.root/'ready').exists())
        p.terminate()
        self.assertEqual(p.wait(timeout=2), 2)

    def test_expected_shell_identity_rejects_pre_startup_reparenting(self):
        with patch.dict(os.environ, {'SCREENSAFE_PARENT_PID': str(os.getpid())}):
            with self.assertRaises(RuntimeError):
                with ParentLifetime():
                    self.fail('a different parent was accepted')


if __name__ == '__main__':
    unittest.main()
