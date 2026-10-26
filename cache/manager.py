#!/usr/bin/env python3
"""Trusted local prepared-loop controller. Packs never supply commands or approvals.

The codec/GPU worker sees only an immutable normalized video, system libraries,
one render node and a private packet socket. The confined Rust renderer alone
connects to Wayland. Live playback is the default and every failure's fallback.
"""
import argparse
import contextlib
import fcntl
import hashlib
import json
import math
import os
from pathlib import Path
import re
import selectors
import signal
import socket
import shutil
import stat
import struct
import subprocess
import sys
import tempfile
import threading
import time

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from screensafe_runtime import ParentLifetime, sandbox_binary, media_launcher, media_process_filter

MAGIC = 0x31435053
VERSION = 1
MAX_VIDEO = 128 << 20
STOP = False
COMMON = ['pack', 'theme', 'branding', 'logo', 'badge', 'photos', 'videos', 'settings']


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(',', ':'), allow_nan=False).encode()


def sha(path):
    with open(path, 'rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def ac_power():
    """Unknown power state never authorizes preparation."""
    supplies = Path('/sys/class/power_supply')
    external = []
    batteries = []
    for p in sorted(supplies.glob('*'))[:16]:
        try:
            kind = (p/'type').read_text().strip()
            if kind == 'Battery':
                batteries.append((p/'status').read_text().strip())
            elif (p/'online').exists():
                external.append((p/'online').read_text().strip() == '1')
        except OSError:
            pass
    return any(external) or bool(batteries and all(s in ['Charging', 'Full'] for s in batteries))


def on_battery():
    if ac_power():
        return False
    for p in Path('/sys/class/power_supply').glob('*/status'):
        try:
            if p.read_text().strip() == 'Discharging':
                return True
        except OSError:
            pass
    return False


def arguments(args, command):
    out = [str(args.renderer), command]
    for name in COMMON:
        value = getattr(args, name, None)
        if value is not None:
            out += ['--'+name, value]
    if command == 'run':
        out += ['--overlay', args.overlay, '--role', args.role]
        if args.exit_after is not None:
            out += ['--exit-after', str(args.exit_after)]
    return out


def small_output(command, timeout=10):
    with tempfile.TemporaryFile() as out, tempfile.TemporaryFile() as err:
        p = subprocess.Popen(command, stdout=out, stderr=err, stdin=subprocess.DEVNULL, start_new_session=True)
        try:
            deadline = time.monotonic()+timeout
            while p.poll() is None:
                if STOP:
                    raise InterruptedError('ScreenSafe helper interrupted')
                if time.monotonic() >= deadline:
                    raise subprocess.TimeoutExpired(command, timeout)
                time.sleep(.05)
            out.seek(0); data = out.read(65537)
            if p.returncode or len(data) > 65536:
                raise ValueError('bounded helper failed')
            return data
        finally:
            terminate(p)


def plan(args):
    result = json.loads(small_output(arguments(args, 'cache-plan')))
    if not isinstance(result, dict) or not re.fullmatch('[a-f0-9]{64}', result.get('fingerprint', '')):
        raise ValueError('invalid preparation plan')
    if result.get('fps') != 24 or not 16 <= result.get('seconds', 0) <= 32:
        raise ValueError('unbounded preparation plan')
    return result


def geometry():
    rows = json.loads(small_output(['/usr/bin/hyprctl', '-j', 'monitors'], 3))
    if not isinstance(rows, list) or len(rows) != 1:
        raise ValueError('one display is required for prepared playback')
    m = rows[0]
    if not isinstance(m, dict):
        raise ValueError('invalid display description')
    w, h = m.get('width', 0), m.get('height', 0)
    if (not isinstance(w, int) or not isinstance(h, int) or not 16 <= w <= 4096 or
        not 16 <= h <= 2160 or not 1 <= m.get('refreshRate', 0) <= 64 or m.get('transform', 0) != 0):
        raise ValueError('display is outside the prepared profile')
    return [w, h]


def devices():
    return [p for p in sorted(Path('/dev/dri').glob('renderD*'))
            if re.fullmatch('renderD[0-9]+', p.name) and p.is_char_device() and os.access(p, os.R_OK | os.W_OK)][:4]


def profile(args, size):
    paths = {str(Path(args.renderer).resolve()), str(Path(args.worker).resolve()), str(Path(__file__).resolve()),
             str(Path(__file__).resolve().parents[1]/'screensafe_runtime.py'), sandbox_binary(), media_launcher(),
             str(Path(sys.executable).resolve()), '/usr/bin/ffmpeg', '/usr/bin/ffprobe'}
    # Include actual dynamically loaded graphics/codec code, beyond package names.
    for executable in [str(args.worker), sandbox_binary()]:
        dependencies = small_output(['/usr/bin/ldd', executable]).decode()
        for word in dependencies.split():
            if word.startswith('/') and Path(word).is_file():
                paths.add(str(Path(word).resolve()))
    drivers = set()
    for directory in ['/usr/lib/dri', '/usr/lib/gbm']:
        for p in Path(directory).glob('*.so'):
            drivers.add(str(p.resolve()))
    paths.update(drivers)
    # Drivers are loaded dynamically and need not appear in the worker's
    # ldd closure (for example Mesa's LLVM dependency). Resolve aliases once.
    for p in sorted(drivers):
        for word in small_output(['/usr/bin/ldd', p]).decode().split():
            if word.startswith('/') and Path(word).is_file():
                paths.add(str(Path(word).resolve()))
    packages = small_output(['/usr/bin/pacman', '-Q', 'mpv', 'ffmpeg', 'mesa', 'libva', 'wayland', 'libseccomp', 'libcap', 'python']).decode()
    return {'version': VERSION, 'files': {p: sha(p) for p in sorted(paths)},
            'packages': packages, 'kernel': os.uname().release, 'size': size,
            'cadence': 'ordered/no-catch-up/24fps/64Hz-approval', 'composition': 'opaque/fixed/no-overlay'}


def identity(base, device, decoder):
    p = Path('/sys/class/drm', device.name, 'device')
    ids = {}
    for name in ['vendor', 'device', 'revision']:
        try:
            ids[name] = (p/name).read_text().strip()
        except OSError:
            ids[name] = ''
    return dict(base, device=str(device), gpu=ids, decoder=decoder)


def cache_key(plan, profile):
    return hashlib.sha256(encoded({'source': plan, 'profile': profile})).hexdigest()


@contextlib.contextmanager
def store(path):
    path.mkdir(mode=0o700, parents=True, exist_ok=True)
    fd = os.open(path, os.O_RDONLY | os.O_DIRECTORY | os.O_CLOEXEC | os.O_NOFOLLOW)
    try:
        info = os.fstat(fd)
        if info.st_uid != os.getuid() or stat.S_IMODE(info.st_mode) != 0o700:
            raise ValueError('prepared directory must be private and user-owned')
        yield fd
    finally:
        os.close(fd)


@contextlib.contextmanager
def snapshot(path):
    source = os.open(path, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC)
    target = None
    try:
        s = os.fstat(source)
        if not stat.S_ISREG(s.st_mode) or not 0 < s.st_size <= MAX_VIDEO:
            raise ValueError('invalid prepared video size/type')
        target = os.memfd_create('screensafe-prepared', os.MFD_CLOEXEC | os.MFD_ALLOW_SEALING)
        count, h = 0, hashlib.sha256()
        while block := os.read(source, 1 << 20):
            count += len(block)
            if count > MAX_VIDEO:
                raise ValueError('prepared video grew beyond limit')
            h.update(block)
            v = memoryview(block)
            while v:
                n = os.write(target, v)
                if n <= 0:
                    raise ValueError("snapshot write made no progress")
                v = v[n:]
        if not count:
            raise ValueError('empty prepared video')
        seals = fcntl.F_SEAL_WRITE | fcntl.F_SEAL_GROW | fcntl.F_SEAL_SHRINK | fcntl.F_SEAL_SEAL
        fcntl.fcntl(target, fcntl.F_ADD_SEALS, seals)
        os.lseek(target, 0, os.SEEK_SET)
        yield target, h.hexdigest(), count
    finally:
        os.close(source)
        if target is not None:
            os.close(target)


def read_record(directory, key):
    fd = os.open(key+'.json', os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK | os.O_CLOEXEC, dir_fd=directory)
    with os.fdopen(fd, 'rb') as f:
        s = os.fstat(f.fileno())
        if not stat.S_ISREG(s.st_mode) or s.st_uid != os.getuid() or s.st_mode & 0o077:
            raise ValueError('untrusted approval permissions')
        data = f.read((128 << 10)+1)
        if len(data) > 128 << 10:
            raise ValueError('approval too large')
    record = json.loads(data)
    if not isinstance(record, dict):
        raise ValueError("approval must be an object")
    return record


def sandbox(args, command, *, media=None, device=None, output=None, policy=None):
    a = [sandbox_binary(), '--unshare-user', '--unshare-all', '--disable-userns',
         '--die-with-parent', '--new-session', '--cap-drop', 'ALL', '--clearenv',
         '--setenv', 'PATH', '/usr/bin', '--setenv', 'HOME', '/tmp', '--setenv', 'LANG', 'C',
         '--setenv', 'OMP_NUM_THREADS', '1']
    for p in ['/usr', '/lib', '/lib64', '/etc/ld.so.cache', '/etc/alternatives']:
        if Path(p).exists():
            a += ['--ro-bind', p, p]
    a += ['--proc', '/proc', '--dev', '/dev', '--size', '67108864', '--tmpfs', '/tmp', '--chdir', '/tmp']
    if media is not None:
        os.lseek(media, 0, os.SEEK_SET)
        a += ['--ro-bind-data', str(media), '/input.mp4']
    if device is not None:
        a += ['--dev-bind', str(device), '/dev/dri/renderD128', '--ro-bind', '/sys', '/sys',
              '--ro-bind', str(args.worker), '/worker']
    if output is not None:
        a += ['--bind', str(output), '/output']
    if policy is not None:
        a += ['--seccomp', str(policy)]
    return a + command


def terminate(p):
    if p.poll() is None:
        try:
            os.killpg(p.pid, signal.SIGTERM)
            p.wait(timeout=3)
        except subprocess.TimeoutExpired:
            os.killpg(p.pid, signal.SIGKILL); p.wait(timeout=3)


def check_preparation():
    if STOP or not ac_power():
        raise ValueError('preparation cancelled or external power disconnected')


def wait_preparation(processes, deadline):
    while any(p.poll() is None for p in processes):
        check_preparation()
        if time.monotonic() > deadline:
            raise ValueError('preparation deadline exceeded')
        time.sleep(0.1)
    if any(p.returncode for p in processes):
        raise ValueError('preparation helper failed')


def encode(args, directory, size, seconds, source_digest):
    w, h = size; frames = math.ceil(seconds*24)
    source = arguments(args, 'prepare-frames') + ['--seed', '1', '--size', f'{w}x{h}',
        '--fps', '24', '--seconds', str(seconds)]
    ff = ['/usr/bin/ffmpeg', '-v', 'error', '-nostdin', '-threads', '2', '-filter_threads', '1',
        '-f', 'rawvideo', '-pixel_format', 'rgba', '-video_size', f'{w}x{h}', '-framerate', '24', '-i', 'pipe:0',
        '-vf', 'scale=in_range=full:out_range=tv:out_color_matrix=bt709,setsar=1,setparams=range=limited:color_primaries=bt709:color_trc=iec61966-2-1:colorspace=bt709',
        '-frames:v', str(frames), '-an', '-c:v', 'libx264', '-preset', 'veryfast', '-crf', '18',
        '-pix_fmt', 'yuv420p', '-threads', '2', '-bf', '0', '-g', '24', '-movflags', '+faststart',
        '-fs', str(MAX_VIDEO), '-f', 'mp4', '/output/video.mp4']
    with (directory/'source.log').open('wb') as e1, (directory/'encoder.log').open('wb') as e2:
        a = subprocess.Popen(source, stdout=subprocess.PIPE, stderr=e1, stdin=subprocess.DEVNULL, start_new_session=True)
        b = None
        try:
            with media_process_filter() as policy:
                b = subprocess.Popen([media_launcher()] + sandbox(args, ff, output=directory, policy=policy),
                    pass_fds=(policy,), stdin=a.stdout, stdout=subprocess.DEVNULL, stderr=e2, start_new_session=True)
            a.stdout.close()
            wait_preparation([a, b], time.monotonic()+300)
        finally:
            a.stdout.close()
            terminate(a)
            if b is not None:
                terminate(b)
    with (directory/'source.log').open('rb') as log:
        metadata = log.read(4097)
    if len(metadata) > 4096 or json.loads(metadata.splitlines()[-1]).get('sourceDigest') != source_digest:
        raise ValueError('rendered source did not match preparation plan')
    return frames


def capture(args, fd, device, decoder, size, frames, directory, label):
    """Verify the same exported framebuffer used in playback, over two loops.

    A software-decoded GPU reference also detects decoder fidelity problems.
    Hashes compare both loops and candidate decoder modes byte for byte.
    """
    w, h = size
    command = ['/worker', 'verify', str(w), str(h), str(frames), '24', decoder]
    # No bulk pixels pass through Python. Rust consumes the worker pipe directly,
    # checks both full loops and returns only a bounded verdict and pixel digest.
    errors = bytearray()
    with tempfile.TemporaryFile() as verdict_file:
        p = subprocess.Popen(sandbox(args, command, media=fd, device=device), pass_fds=(fd,),
            stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE, start_new_session=True)
        def drain_errors():
            while block := p.stderr.read(4096):
                errors.extend(block)
                if len(errors) > 65536:
                    terminate(p)
                    return
        reader = threading.Thread(target=drain_errors, daemon=True)
        reader.start()
        try:
            guard = subprocess.Popen([str(args.renderer), 'approve-frames', '--size', f'{w}x{h}', '--frames', str(frames)],
                stdin=p.stdout, stdout=verdict_file, stderr=(directory/(label+'-guard.log')).open('wb'), start_new_session=True)
            p.stdout.close()
        except BaseException:
            terminate(p); reader.join(timeout=2)
            raise
        try:
            wait_preparation([p, guard], time.monotonic()+600)
            verdict_file.seek(0); result = verdict_file.read(4097)
            reader.join(timeout=2)
            if len(result) > 4096 or len(errors) > 65536:
                raise ValueError('GPU verification output limit exceeded')
            verdict = json.loads(result)
            pixels = verdict.get('pixelsBlake3', '')
            if verdict.get('approved') is not True or not re.fullmatch('[a-f0-9]{64}', pixels):
                raise ValueError('GPU repeat or flash approval failed')
            return pixels, verdict
        finally:
            terminate(p); terminate(guard)
            reader.join(timeout=2)
            (directory/(label+'-gpu.log')).write_bytes(errors[:65536])


def available(args):
    try:
        sandbox_binary()
    except (OSError, ValueError):
        return False
    return (args.worker.is_file() and os.access(args.worker, os.X_OK) and
            all(Path(p).is_file() for p in ['/usr/bin/ffmpeg', '/usr/bin/hyprctl']) and bool(devices()))


def lookup(args, p, base, directory):
    for device in devices():
        for decoder in ['copy', 'software']:
            ident = identity(base, device, decoder); key = cache_key(p, ident)
            try:
                record = read_record(directory, key)
                verdict = record.get('verdict')
                expected_frames = math.ceil(p['seconds']*24)
                if (record.get('version') != VERSION or record.get('source') != p or record.get('profile') != ident or
                        not isinstance(verdict, dict) or verdict.get('approved') is not True or
                        record.get('fps') != 24 or record.get('frames') != expected_frames or
                        verdict.get('frames') != expected_frames or verdict.get('tested_rate') != 64 or
                        not re.fullmatch('[a-f0-9]{64}', record.get('sha256', ''))):
                    continue
                return key, record, device, decoder
            except (OSError, ValueError, KeyError, TypeError):
                pass
    return None


def evict(args, needed=0):
    files = [p for p in args.root.glob('*.mp4') if re.fullmatch(r'[a-f0-9]{64}\.mp4', p.name) and not p.is_symlink()]
    total = sum(p.stat().st_size + (p.with_suffix('.json').stat().st_size if p.with_suffix('.json').is_file() and not p.with_suffix('.json').is_symlink() else 0) for p in files)
    for p in sorted(files, key=lambda p: p.stat().st_mtime_ns):
        if total+needed <= args.limit << 20:
            break
        metadata = p.with_suffix('.json')
        size = p.stat().st_size + (metadata.stat().st_size if metadata.is_file() and not metadata.is_symlink() else 0)
        p.unlink(); metadata.unlink(missing_ok=True); total -= size
    if total+needed > args.limit << 20:
        raise ValueError('prepared storage limit exceeded')


def prepare(args):
    check_preparation()
    if not available(args):
        raise ValueError('prepared playback is unavailable on this installation')
    with store(args.root) as directory:
        lock = os.open('.lock', os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=directory)
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            p = plan(args)
            if p['eligible'] is not True:
                raise ValueError(p.get('reason', 'screensaver needs live playback'))
            size = geometry(); base = profile(args, size)
            evict(args, MAX_VIDEO)
            if shutil.disk_usage(args.root).free < 256 << 20:
                raise ValueError("insufficient free space for bounded preparation")
            with tempfile.TemporaryDirectory(prefix='.prepare-', dir=args.root) as work:
                work = Path(work); frames = encode(args, work, size, p['seconds'], p['fingerprint'])
                # Recheck source and display before issuing approval. Concurrent
                # edits may cancel a build, never approve it under an old key.
                if plan(args) != p or geometry() != size:
                    raise ValueError('screensaver or display changed during preparation')
                with snapshot(work/'video.mp4') as (fd, media_sha, length):
                    chosen = None
                    for device in devices():
                        try:
                            reference, verdict = capture(args, fd, device, 'software', size, frames, work, 'reference')
                            chosen = (device, 'software', verdict)
                            try:
                                candidate, candidate_verdict = capture(args, fd, device, 'copy', size, frames, work, 'copy')
                                if candidate == reference:
                                    chosen = (device, 'copy', candidate_verdict)
                            except (OSError, ValueError, subprocess.SubprocessError):
                                check_preparation()
                            break
                        except (OSError, ValueError, subprocess.SubprocessError):
                            check_preparation()
                    if chosen is None:
                        raise ValueError('no GPU profile passed verification')
                    check_preparation()
                    if plan(args) != p or geometry() != size or profile(args, size) != base:
                        raise ValueError('preparation inputs changed')
                    device, decoder, verdict = chosen; ident = identity(base, device, decoder); key = cache_key(p, ident)
                    record = {'version': VERSION, 'source': p, 'profile': ident, 'sha256': media_sha,
                              'bytes': length, 'frames': frames, 'fps': 24, 'verdict': verdict}
                    evict(args, length + len(encoded(record)))
                    os.chmod(work/'video.mp4', 0o600)
                    (work/'approval.json').write_bytes(encoded(record)); os.chmod(work/'approval.json', 0o600)
                    os.replace(work/'video.mp4', args.root/(key+'.mp4'))
                    os.replace(work/'approval.json', args.root/(key+'.json'))
            print(json.dumps({'state': 'ready', 'bytes': length, 'decoder': decoder}))
        finally:
            os.close(lock)


def status(args):
    if not available(args):
        return {'state': 'unavailable'}
    p = plan(args)
    if p['eligible'] is not True:
        return {'state': 'ineligible'}
    with store(args.root) as directory:
        found = lookup(args, p, profile(args, geometry()), directory)
        total = sum(q.stat().st_size for q in args.root.glob('*.mp4') if not q.is_symlink())
        return {'state': 'ready' if found else 'empty', 'bytes': total}


def clear(args, remove_all=True):
    with store(args.root) as directory:
        lock = os.open('.lock', os.O_WRONLY | os.O_CREAT | os.O_NOFOLLOW | os.O_CLOEXEC, 0o600, dir_fd=directory)
        try:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            if not remove_all:
                evict(args)
                return
            for p in args.root.iterdir():
                if re.fullmatch(r'[a-f0-9]{64}\.(mp4|json)', p.name) and p.is_file() and not p.is_symlink():
                    p.unlink()
        finally:
            os.close(lock)


def live(args, overlay=None):
    if overlay is not None:
        args.overlay = overlay
    os.execve(args.renderer, arguments(args, 'run'), dict(os.environ))


def overlay_enabled(value):
    fields = {}
    for part in value.split(','):
        if not part:
            continue
        name, number = part.split('=', 1)
        if name in fields:
            raise ValueError('duplicate overlay field')
        number = float(number)
        if not math.isfinite(number):
            raise ValueError('invalid overlay number')
        fields[name] = number
    return fields.get('enabled', 0) != 0


def run(args):
    if (args.battery_only and not on_battery()) or overlay_enabled(args.overlay):
        live(args)
    if not available(args):
        live(args)
    with store(args.root) as directory:
        p = plan(args)
        if not p['eligible']:
            live(args)
        base = profile(args, geometry()); found = lookup(args, p, base, directory)
        if not found:
            live(args)
        key, record, device, decoder = found
        with snapshot(args.root/(key+'.mp4')) as (fd, media_sha, length):
            if media_sha != record['sha256'] or length != record['bytes']:
                live(args)
            os.utime(args.root/(key+'.mp4'), None, follow_symlinks=False)
            a, b = socket.socketpair(socket.AF_UNIX, socket.SOCK_SEQPACKET)
            w, h = base['size']; frames = record['frames']
            renderer_cmd = [str(args.renderer), 'run-prepared', '--size', f'{w}x{h}', '--frames', str(frames),
                '--fps', '24', '--role', args.role]
            if args.exit_after is not None:
                renderer_cmd += ['--exit-after', str(args.exit_after)]
            worker_cmd = ['/worker', 'serve', str(w), str(h), str(frames), '24', decoder]
            worker = subprocess.Popen(sandbox(args, worker_cmd, media=fd, device=device), pass_fds=(fd,),
                stdin=b, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, start_new_session=True)
            try:
                renderer = subprocess.Popen(renderer_cmd, stdin=a, start_new_session=True)
            except BaseException:
                a.close(); b.close(); terminate(worker)
                raise
            a.close(); reconfigure = None
            selector = selectors.DefaultSelector()
            try:
                try:
                    selector.register(sys.stdin, selectors.EVENT_READ)
                except (OSError, ValueError):
                    pass
                pending = bytearray(); next_power = time.monotonic()+2; asked_quit = False
                while renderer.poll() is None and not STOP:
                    if worker.poll() is not None:
                        try:
                            renderer.wait(timeout=0.3)
                        except subprocess.TimeoutExpired:
                            pass
                        break
                    if args.battery_only and time.monotonic() >= next_power:
                        next_power = time.monotonic()+2
                        if not on_battery():
                            reconfigure = args.overlay; break
                    for _, _ in selector.select(0.1):
                        chunk = os.read(0, 1024)
                        if not chunk:
                            selector.unregister(sys.stdin)
                            if args.managed:
                                return 0
                            continue
                        pending.extend(chunk)
                        if len(pending) > 8192:
                            raise ValueError('control input exceeded limit')
                        while b'\n' in pending:
                            line, _, pending = pending.partition(b'\n')
                            if line == b'quit':
                                asked_quit = True
                                b.send(struct.pack('<4I', MAGIC, 4, 0, 0))
                            elif line.startswith(b'overlay '):
                                value = line[8:].decode('ascii')
                                if value != args.overlay:
                                    reconfigure = value
                    if asked_quit:
                        return 0
                    if reconfigure is not None:
                        break
                if STOP:
                    return 143
                if reconfigure is None and renderer.poll() == 0:
                    return 0
            finally:
                selector.close(); terminate(renderer); terminate(worker); b.close()
    live(args, reconfigure)


def main():
    global STOP
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('action', choices=['prepare', 'status', 'clear', 'trim', 'run'])
    for name in ['renderer', 'worker', 'root']:
        parser.add_argument('--'+name, type=Path, required=True)
    for name in COMMON:
        parser.add_argument('--'+name)
    parser.add_argument('--overlay', default='enabled=0')
    parser.add_argument('--role', default='org.omarchy.screensaver')
    parser.add_argument('--limit', type=int, choices=[256, 512, 1024, 2048], default=512)
    parser.add_argument('--battery-only', type=int, choices=[0, 1], default=1)
    parser.add_argument('--exit-after', type=float)
    parser.add_argument('--managed', action='store_true', help='end playback when the shell control pipe closes')
    args = parser.parse_args()
    def stop(*unused):
        global STOP
        STOP = True
        if args.action not in ('run', 'prepare'):
            raise InterruptedError('ScreenSafe short command interrupted')
    signal.signal(signal.SIGTERM, stop); signal.signal(signal.SIGINT, stop)
    try:
        if args.action == 'run':
            try:
                return run(args)
            except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
                if not STOP:
                    live(args)
                return 143
        if args.action == 'prepare':
            os.nice(10)
            prepare(args)
        elif args.action in ['clear', 'trim']:
            clear(args, args.action == 'clear')
            print('{"state":"empty"}')
        else:
            print(json.dumps(status(args)))
        return 0
    except (OSError, ValueError, KeyError, TypeError, subprocess.SubprocessError):
        print(json.dumps({'state': 'cancelled' if STOP else 'unavailable'}))
        return 2


def cli():
    # Install the lifetime guard before parsing or starting any stage. Its
    # signal requests graceful cleanup; a bounded emergency exit also ends
    # the parent-bound renderer and sandbox if cleanup itself stops responding.
    def parent_stopped(*unused):
        global STOP
        STOP = True
    signal.signal(signal.SIGTERM, parent_stopped)
    signal.signal(signal.SIGINT, parent_stopped)
    with ParentLifetime():
        return main()


if __name__ == '__main__':
    sys.exit(cli())
