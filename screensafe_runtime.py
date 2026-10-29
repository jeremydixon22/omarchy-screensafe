"""Trusted Linux runtime support; no pack-supplied commands or policy inputs."""
import contextlib
import ctypes
import errno
import fcntl
import os
from pathlib import Path
import platform
import select
import signal
import threading


def sandbox_binary():
    arch = platform.machine()
    if arch not in ('x86_64', 'aarch64'):
        raise ValueError('unsupported ScreenSafe architecture')
    path = Path(__file__).resolve().parent/'bin'/('screensafe-sandbox-'+arch)
    if not path.is_file() or not os.access(path, os.X_OK):
        raise ValueError('the bundled ScreenSafe sandbox is missing')
    return str(path)


def media_launcher():
    path = Path(__file__).resolve().parent/'bin'/('screensafe-media-'+platform.machine())
    if not path.is_file() or not os.access(path, os.X_OK):
        raise ValueError('the bundled ScreenSafe media launcher is missing')
    return str(path)


@contextlib.contextmanager
def media_process_filter():
    """Allow threads, never extra processes, in the external-media cage.

    Bubblewrap installs this before the initial FFmpeg/ffprobe exec. execve
    remains necessary for that launch; replacing the same already-confined
    process cannot reset its CPU limit, expand mounts or create descendants.
    execveat and io_uring are denied. No media bytes or author options influence
    this fixed policy. New sockets/connections are denied independently of the
    private network namespace; file conversion and GPU readback need none.
    libseccomp is supplied by Omarchy's base systemd stack.
    """
    class Compare(ctypes.Structure):
        _fields_ = [('arg', ctypes.c_uint), ('op', ctypes.c_int),
                    ('a', ctypes.c_uint64), ('b', ctypes.c_uint64)]
    lib = ctypes.CDLL('libseccomp.so.2', use_errno=True)
    lib.seccomp_init.argtypes = [ctypes.c_uint32]
    lib.seccomp_init.restype = ctypes.c_void_p
    lib.seccomp_release.argtypes = [ctypes.c_void_p]
    lib.seccomp_release.restype = None
    lib.seccomp_syscall_resolve_name.argtypes = [ctypes.c_char_p]
    lib.seccomp_syscall_resolve_name.restype = ctypes.c_int
    lib.seccomp_rule_add_array.argtypes = [ctypes.c_void_p, ctypes.c_uint32, ctypes.c_int,
                                           ctypes.c_uint, ctypes.POINTER(Compare)]
    lib.seccomp_export_bpf.argtypes = [ctypes.c_void_p, ctypes.c_int]
    ctx = lib.seccomp_init(0x7fff0000)  # SCMP_ACT_ALLOW
    if not ctx:
        raise ValueError('cannot create media process policy')
    fd = None
    def rule(name, error, comparison=None):
        nr = lib.seccomp_syscall_resolve_name(name.encode())
        # AArch64 has no fork/vfork syscall: those use clone, independently
        # restricted below. Negative libseccomp pseudo numbers are not native.
        if nr < 0:
            return
        pointer = ctypes.pointer(comparison) if comparison is not None else None
        if lib.seccomp_rule_add_array(ctx, 0x00050000 | error, nr, int(pointer is not None), pointer) < 0:
            raise ValueError('cannot compile media process policy')
    try:
        for name in ('fork', 'vfork', 'execveat', 'io_uring_setup', 'ptrace',
                     'process_vm_readv', 'process_vm_writev', 'unshare', 'setns',
                     'socket', 'socketpair', 'connect', 'bind', 'listen', 'accept', 'accept4'):
            rule(name, errno.EPERM)
        rule('clone3', errno.ENOSYS)  # pthread falls back to inspectable clone flags.
        rule('clone', errno.EPERM, Compare(0, 7, 0x10000, 0))  # no CLONE_THREAD
        fd = os.memfd_create('screensafe-media-policy', os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
        if lib.seccomp_export_bpf(ctx, fd) < 0 or not 0 < os.fstat(fd).st_size <= 32768:
            raise ValueError('cannot export bounded media process policy')
        fcntl.fcntl(fd, fcntl.F_ADD_SEALS,
                    fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL)
        os.lseek(fd, 0, os.SEEK_SET)
        yield fd
    finally:
        lib.seccomp_release(ctx)
        if fd is not None:
            os.close(fd)


class ParentLifetime:
    """Follow the exact parent process, with no PID-reuse race or idle polling.

    Normal CLI use can have a closed stdin. A pidfd independently propagates
    shell/terminal death through the Python controller to its confined children.
    The normal SIGTERM handler gets three seconds to clean up before emergency
    exit. Rust children and Bubblewrap independently bind their own parent death.
    """
    def __enter__(self):
        parent = os.getppid()
        expected = os.environ.get('SCREENSAFE_PARENT_PID')
        if expected is not None and (not expected.isdecimal() or int(expected) != parent):
            raise RuntimeError('ScreenSafe shell exited before controller startup')
        if parent == 1:
            raise RuntimeError('ScreenSafe controller has no live parent')
        self.parent = os.pidfd_open(parent, 0)
        self.read, self.write = os.pipe2(os.O_CLOEXEC)
        if os.getppid() != parent:
            os.close(self.parent); os.close(self.read); os.close(self.write)
            raise RuntimeError('ScreenSafe parent exited during startup')
        self.thread = threading.Thread(target=self.watch, daemon=True)
        self.thread.start()
        return self

    def watch(self):
        ready, _, _ = select.select([self.parent, self.read], [], [])
        if self.read in ready:
            return
        os.kill(os.getpid(), signal.SIGTERM)
        ready, _, _ = select.select([self.read], [], [], 3)
        if not ready:
            os._exit(143)

    def __exit__(self, *unused):
        os.write(self.write, b'x')
        self.thread.join(timeout=1)
        os.close(self.parent); os.close(self.read); os.close(self.write)
