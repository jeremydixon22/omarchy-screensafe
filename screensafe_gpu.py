"""Trusted PCI render-node authority shared by the video and prepared decoders."""
import os
from pathlib import Path
import re
import stat

# Bubblewrap resolves device-bind paths during setup. Check the mounted inode
# before the codec sees media, including hot-unplug/replacement races. This
# trusted bootstrap replaces itself; it cannot reset the native resource caps.
VERIFY_DEVICE = '''import os,stat,sys
s=os.stat(sys.argv[1],follow_symlinks=False)
if not stat.S_ISCHR(s.st_mode) or (s.st_dev,s.st_ino,s.st_rdev)!=tuple(map(int,sys.argv[2:5])):
    sys.exit("screensafe-video: GPU device identity changed")
os.execv(sys.argv[5],sys.argv[5:])
'''


class RenderNode:
    """One retained DRM render-node identity, eight pinned metadata files.

    This hardware profile supports PCI render nodes, without choosing
    a vendor driver or exposing primary/card nodes, all of /sys, or a display.
    Render-node ioctls still add kernel/driver authority; this is not a claim
    of isolation from GPU bugs or global GPU memory/engine exhaustion.
    """
    def __init__(self, path):
        self.path=Path(path);self.fds=[];self.metadata=[]
        try:
            if self.path.parent!=Path('/dev/dri') or not re.fullmatch(r'renderD[0-9]+',self.path.name):
                raise ValueError('GPU access requires an explicit /dev/dri/renderD* node')
            directory=os.open('/dev/dri',os.O_PATH|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
            try:
                parent=os.fstat(directory)
                if parent.st_uid!=0 or parent.st_mode&0o022:
                    raise ValueError('GPU device directory must be root-owned and not writable by other users')
                node=os.open(self.path.name,os.O_PATH|os.O_NOFOLLOW|os.O_CLOEXEC,dir_fd=directory)
            finally:os.close(directory)
            self.fds.append(node);s=os.fstat(node)
            minor=os.minor(s.st_rdev)
            if (not stat.S_ISCHR(s.st_mode) or os.major(s.st_rdev)!=226 or not 128<=minor<=255
                    or self.path.name!=f'renderD{minor}'):
                raise ValueError('GPU selection is not a DRM render node')
            self.identity=(s.st_dev,s.st_ino,s.st_rdev)
            self.char=Path(f'/sys/dev/char/226:{minor}')
            self.render=self.char.resolve(strict=True)
            self.device=(self.char/'device').resolve(strict=True)
            if (not self.device.is_relative_to('/sys/devices') or self.device==Path('/sys/devices')
                    or self.render!=self.device/'drm'/self.path.name
                    or (self.device/'subsystem').resolve(strict=True)!=Path('/sys/bus/pci')):
                raise ValueError('this GPU profile requires PCI render-node metadata')
            for base,names in [(self.device,['vendor','device','revision','subsystem_vendor','subsystem_device','uevent']),
                               (self.render,['dev','uevent'])]:
                for name in names:
                    destination=base/name
                    fd=os.open(destination,os.O_RDONLY|os.O_NOFOLLOW|os.O_NONBLOCK|os.O_CLOEXEC)
                    self.fds.append(fd);meta=os.fstat(fd)
                    if not stat.S_ISREG(meta.st_mode) or meta.st_size>65536:
                        raise ValueError('invalid bounded GPU metadata file')
                    self.metadata.append((fd,destination))
        except BaseException:
            self.close();raise

    def validate_current(self):
        if not self.fds:
            raise ValueError('GPU authority is already closed')
        info=os.stat(self.path,follow_symlinks=False)
        if not stat.S_ISCHR(info.st_mode) or (info.st_dev,info.st_ino,info.st_rdev)!=self.identity:
            raise ValueError('GPU device identity changed')

    def profile(self):
        """Stable PCI identity from pinned files, independent of boot inode IDs."""
        self.validate_current()
        ids={}
        names={'vendor','device','revision','subsystem_vendor','subsystem_device'}
        for fd,path in self.metadata:
            if path.parent==self.device and path.name in names:
                raw=os.pread(fd,129,0)
                if len(raw)>128:
                    raise ValueError('GPU PCI identity exceeded its limit')
                value=raw.decode('ascii').strip()
                if not re.fullmatch(r'0x[0-9a-fA-F]{1,8}',value):
                    raise ValueError('invalid GPU PCI identity')
                ids[path.name]=value.lower()
        if set(ids)!=names:
            raise ValueError('incomplete GPU PCI identity')
        return {'device':str(self.path),'pciPath':str(self.device),'gpu':ids}

    def mounts(self):
        if not self.fds:raise ValueError('GPU authority is already closed')
        args=['--dev-bind',str(self.path),str(self.path),'--dir','/sys/class/drm','--dir','/sys/bus/pci',
            '--dir',str(self.render),'--symlink',str(self.device),str(self.render/'device'),
            '--symlink','/sys/class/drm',str(self.render/'subsystem'),
            '--symlink','/sys/bus/pci',str(self.device/'subsystem'),
            '--symlink',str(self.render),str(self.char),
            '--symlink',str(self.render),str(Path('/sys/class/drm')/self.path.name)]
        for fd,path in self.metadata:args+=['--ro-bind-fd',str(fd),str(path)]
        return args

    @property
    def pass_fds(self):
        # The retained O_PATH device handle is parent-only. Only descriptors
        # consumed by Bubblewrap's metadata mounts cross the exec boundary.
        return tuple(fd for fd,_ in self.metadata)

    def command(self, command):
        return ['/usr/bin/python3','-I','-c',VERIFY_DEVICE,str(self.path),*map(str,self.identity),*command]

    def close(self):
        for fd in self.fds:os.close(fd)
        self.fds.clear();self.metadata.clear()

    def __enter__(self):return self
    def __exit__(self,*unused):self.close()


def select_node(path):
    """The trusted caller may select an explicit node or root-controlled list."""
    if str(path)!='auto':return RenderNode(path)
    for candidate in sorted(Path('/dev/dri').glob('renderD*'))[:4]:
        try:return RenderNode(candidate)
        except (ValueError,OSError):pass
    raise ValueError('no eligible hardware video device; select software decoding')
