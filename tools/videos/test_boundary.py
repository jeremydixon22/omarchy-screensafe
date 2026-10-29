"""Real cage and hostile-protocol probes. Physical GPU tests require opt-in."""
import array
import errno
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import threading
import unittest

import hardware
import play_v2
import ssv2


PROBE = '''import ctypes,errno,json,os,resource,socket,threading
r={}
for family,name in [(socket.AF_INET,'inet'),(socket.AF_INET6,'inet6'),(socket.AF_UNIX,'unix')]:
    try:s=socket.socket(family,socket.SOCK_STREAM);s.close();r[name]=0
    except OSError as e:r[name]=e.errno
try:
    p=os.fork()
    if not p:os._exit(0)
    os.waitpid(p,0);r['fork']=0
except OSError as e:r['fork']=e.errno
lib=ctypes.CDLL(None,use_errno=True)
r['unshare']=lib.unshare(0x40000000);r['unshareErrno']=ctypes.get_errno()
r['setns']=lib.setns(-1,0);r['setnsErrno']=ctypes.get_errno()
x=[];t=threading.Thread(target=lambda:x.append(True));t.start();t.join();r['thread']=x==[True]
r['status']={a:b.strip() for a,b in (line.split(':',1) for line in open('/proc/self/status') if ':' in line)}
r['hidden']={p:not os.path.exists(p) for p in ['/home','/root','/run/user','/sys/kernel','/sys/firmware']}
r['environment']={k:os.getenv(k) for k in ['HOME','DISPLAY','WAYLAND_DISPLAY','DBUS_SESSION_BUS_ADDRESS','SSH_AUTH_SOCK']}
r['limits']={k:resource.getrlimit(getattr(resource,k)) for k in ['RLIMIT_AS','RLIMIT_CPU','RLIMIT_NOFILE','RLIMIT_CORE','RLIMIT_FSIZE']}
r['tmpBytes']=os.statvfs('/tmp').f_blocks*os.statvfs('/tmp').f_frsize
r['writeErrors']={}
for path in ['/input.mp4','/usr/bin/true']:
    try:fd=os.open(path,os.O_WRONLY);os.close(fd);r['writeErrors'][path]=0
    except OSError as e:r['writeErrors'][path]=e.errno
r['extraFDs']=[]
for fd in os.listdir('/proc/self/fd'):
    try:
        if int(fd)>2:os.fstat(int(fd));r['extraFDs'].append(fd)
    except OSError:pass
r['dri']=os.listdir('/dev/dri') if os.path.isdir('/dev/dri') else []
r['unsafeGpuMetadata']={}
for node in r['dri']:
    for tail in ['device/vendor','dev']:
        path='/sys/class/drm/'+node+'/'+tail
        try:fd=os.open(path,os.O_WRONLY);os.close(fd);r['writeErrors'][path]=0
        except OSError as e:r['writeErrors'][path]=e.errno
    for name in ['resource0','config','rom']:
        path='/sys/class/drm/'+node+'/device/'+name
        r['unsafeGpuMetadata'][path]=os.path.exists(path)
print(json.dumps(r))
'''


def cage_probe(gpu=None):
    with tempfile.NamedTemporaryFile() as source,ssv2.media.media_process_filter() as policy:
        source.write(b'probe');source.flush()
        command=['/usr/bin/python3','-I','-c',PROBE]
        cage=ssv2.media.sandbox(source.fileno(),[],policy)
        if gpu is not None:cage+=gpu.mounts();command=gpu.command(command)
        p=subprocess.run([ssv2.media.media_launcher()]+cage+['--']+command,
            pass_fds=(source.fileno(),policy,*(() if gpu is None else gpu.pass_fds)),capture_output=True,timeout=10)
        if p.returncode:raise AssertionError(p.stderr.decode(errors='replace'))
        return json.loads(p.stdout)


class BoundaryTests(unittest.TestCase):
    def assert_cage(self,r):
        for name in ['inet','inet6','unix','fork','unshareErrno','setnsErrno']:
            self.assertEqual(r[name],errno.EPERM,(name,r))
        self.assertTrue(r['thread']);self.assertTrue(all(r['hidden'].values()))
        self.assertTrue(all(v is None for v in r['environment'].values()))
        self.assertEqual(r['status']['NoNewPrivs'],'1');self.assertEqual(r['status']['Seccomp'],'2')
        self.assertEqual(int(r['status']['CapEff'],16),0)
        self.assertEqual(r['limits'],{'RLIMIT_AS':[2<<30,2<<30],'RLIMIT_CPU':[180,180],
            'RLIMIT_NOFILE':[64,64],'RLIMIT_CORE':[0,0],'RLIMIT_FSIZE':[128<<20,128<<20]})
        self.assertLessEqual(r['tmpBytes'],64<<20)
        self.assertTrue(all(e in [errno.EROFS,errno.EACCES] for e in r['writeErrors'].values()))
        self.assertEqual(r['extraFDs'],[])

    def test_software_cage_denies_host_network_process_and_file_authority(self):
        r=cage_probe();self.assert_cage(r);self.assertEqual(r['dri'],[])

    def test_device_identity_bootstrap_refuses_replacement_and_symlink(self):
        node=Path('/dev/null');s=node.stat();identity=[str(s.st_dev),str(s.st_ino),str(s.st_rdev)]
        command=[sys.executable,'-I','-c',hardware.VERIFY_DEVICE,str(node),*identity,'/usr/bin/true']
        self.assertEqual(subprocess.run(command,capture_output=True,timeout=3).returncode,0)
        command[6]=str(s.st_ino+1)
        self.assertNotEqual(subprocess.run(command,capture_output=True,timeout=3).returncode,0)
        with tempfile.TemporaryDirectory() as directory:
            link=Path(directory)/'device';link.symlink_to(node);command[4]=str(link);command[6]=str(s.st_ino)
            self.assertNotEqual(subprocess.run(command,capture_output=True,timeout=3).returncode,0)
        for path in ['/dev/null','/dev/dri/card0','/dev/dri/by-path/device-render','/tmp/renderD128']:
            with self.assertRaises(ValueError):hardware.RenderNode(path)

    def test_malformed_requests_close_channel_without_leaking_received_fds(self):
        request=play_v2.REQUEST.pack(play_v2.MAGIC,1,0,0)
        cases=[(request,0),(request,2),(request+b'extra',1),(request[:-1],1),
            (play_v2.REQUEST.pack(0,1,0,0),1),(play_v2.REQUEST.pack(play_v2.MAGIC,1,65535,0),1),
            (play_v2.REQUEST.pack(play_v2.MAGIC,1,0,1),1)]
        with tempfile.TemporaryFile() as source:
            for data,count in cases:
                before=len(os.listdir('/proc/self/fd'));server,client=socket.socketpair(socket.AF_UNIX,socket.SOCK_SEQPACKET);errors=[]
                def serve():
                    try:play_v2.serve(server,None)
                    except ValueError as e:errors.append(str(e))
                    finally:server.close()
                t=threading.Thread(target=serve);t.start()
                try:
                    rights=[] if not count else [(socket.SOL_SOCKET,socket.SCM_RIGHTS,array.array('i',[source.fileno()]*count))]
                    client.sendmsg([data],rights);client.settimeout(2)
                    self.assertEqual(client.recv(32),b'')
                    t.join(timeout=2);self.assertFalse(t.is_alive());self.assertTrue(errors)
                finally:client.close();server.close();t.join(timeout=2)
                self.assertEqual(len(os.listdir('/proc/self/fd')),before)

    @unittest.skipUnless(os.environ.get('SCREENSAFE_TEST_RENDER_NODE'),'explicit physical GPU required')
    def test_hardware_cage_has_one_node_and_only_allowlisted_readonly_metadata(self):
        with hardware.RenderNode(os.environ['SCREENSAFE_TEST_RENDER_NODE']) as gpu:
            self.assertTrue(all(not os.get_inheritable(fd) for fd in gpu.fds))
            r=cage_probe(gpu);self.assert_cage(r)
            self.assertEqual(r['dri'],[gpu.path.name]);self.assertFalse(any(r['unsafeGpuMetadata'].values()))


if __name__=='__main__':unittest.main()
