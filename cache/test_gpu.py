"""Pinned GPU identity and real prepared-cage scope checks."""
import errno
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
from types import SimpleNamespace
import unittest

import manager
from screensafe_gpu import RenderNode, VERIFY_DEVICE

ROOT=Path(__file__).resolve().parents[1]


class IdentityTests(unittest.TestCase):
    def test_device_bootstrap_refuses_a_different_character_node_before_exec(self):
        with tempfile.TemporaryDirectory() as directory:
            marker=Path(directory)/'executed'
            source=os.stat('/dev/null',follow_symlinks=False)
            run=[sys.executable,'-I','-c',VERIFY_DEVICE,'/dev/zero',
                 str(source.st_dev),str(source.st_ino),str(source.st_rdev),
                 sys.executable,'-I','-c',f'open({str(marker)!r},"w").close()']
            p=subprocess.run(run,capture_output=True,timeout=5)
            self.assertNotEqual(p.returncode,0)
            self.assertFalse(marker.exists())

    def test_profile_reads_pinned_bounded_metadata_after_path_replacement(self):
        with tempfile.TemporaryDirectory() as directory:
            device=Path(directory)
            gpu=object.__new__(RenderNode)
            gpu.path=Path('/dev/dri/renderD128');gpu.device=device;gpu.metadata=[];gpu.fds=[]
            # Acquisition/replacement refusal is exercised by the shared device
            # bootstrap tests. This fixture isolates retained metadata reads.
            gpu.validate_current=lambda:None
            try:
                for name in ['vendor','device','revision','subsystem_vendor','subsystem_device']:
                    p=device/name;p.write_text('0x1234\n')
                    fd=os.open(p,os.O_RDONLY|os.O_CLOEXEC)
                    gpu.fds.append(fd);gpu.metadata.append((fd,p))
                vendor=device/'vendor';vendor.rename(device/'original-vendor');vendor.write_text('0xabcd\n')
                self.assertEqual(gpu.profile()['gpu']['vendor'],'0x1234')
                (device/'original-vendor').write_text('x'*129)
                with self.assertRaises(ValueError):gpu.profile()
                (device/'original-vendor').write_text('not a PCI ID\n')
                with self.assertRaises(ValueError):gpu.profile()
            finally:gpu.close()


SCOPE='''import errno,json,os,stat
r={'hidden':{p:not os.path.exists(p) for p in ['/home','/root','/run/user','/sys/kernel','/sys/firmware','/sys/devices/system','/sys/bus/usb']}}
r['dri']=os.listdir('/dev/dri');r['readonly']={};r['metadata']=[]
for node in r['dri']:
 for tail in ['device/vendor','device/device','device/revision','dev']:
  path='/sys/class/drm/'+node+'/'+tail
  with open(path,'rb') as f:r['metadata'].append(bool(f.read(128)))
  try:fd=os.open(path,os.O_WRONLY);os.close(fd);r['readonly'][tail]=0
  except OSError as e:r['readonly'][tail]=e.errno
 for tail in ['device/resource0','device/config','device/rom']:
  r['hidden'][tail]=not os.path.exists('/sys/class/drm/'+node+'/'+tail)
r['environment']={k:os.getenv(k) for k in ['DISPLAY','WAYLAND_DISPLAY','DBUS_SESSION_BUS_ADDRESS','SSH_AUTH_SOCK']}
r['extraFDs']=[]
for fd in os.listdir('/proc/self/fd'):
 try:
  if int(fd)>2:os.fstat(int(fd));r['extraFDs'].append(fd)
 except OSError:pass
print(json.dumps(r))
'''


@unittest.skipUnless(os.environ.get('SCREENSAFE_TEST_RENDER_NODE'),'explicit physical GPU required')
class CageTests(unittest.TestCase):
    def test_prepared_cage_exposes_only_pinned_gpu_metadata(self):
        args=SimpleNamespace(worker=ROOT/'bin'/('screensafe-cache-'+os.uname().machine))
        with tempfile.TemporaryDirectory() as directory, RenderNode(os.environ['SCREENSAFE_TEST_RENDER_NODE']) as gpu:
            media=Path(directory)/'media';media.write_bytes(b'fixed probe input')
            with manager.snapshot(media) as (fd,_,_):
                command=manager.sandbox(args,['/usr/bin/python3','-I','-c',SCOPE],media=fd,device=gpu)
                p=subprocess.run(command,pass_fds=(fd,*gpu.pass_fds),capture_output=True,text=True,timeout=10)
            self.assertEqual(p.returncode,0,p.stderr)
            r=json.loads(p.stdout)
            self.assertTrue(all(r['hidden'].values()),r)
            self.assertEqual(r['dri'],[gpu.path.name])
            self.assertTrue(all(r['metadata']))
            self.assertTrue(all(v in [errno.EROFS,errno.EACCES] for v in r['readonly'].values()))
            self.assertTrue(all(v is None for v in r['environment'].values()))
            self.assertEqual(r['extraFDs'],[])


if __name__=='__main__':unittest.main()
