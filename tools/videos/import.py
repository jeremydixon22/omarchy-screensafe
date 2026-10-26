#!/usr/bin/env python3
"""Import one local MP4 into a bounded ScreenSafe frame archive. Prototype, local only."""
import argparse
import json
import math
import os
from pathlib import Path
import selectors
import stat
import struct
import subprocess
import tempfile
import time
import sys

sys.path.insert(0, str(Path(__file__).resolve().parents[2]))
from screensafe_runtime import ParentLifetime, media_process_filter, sandbox_binary, media_launcher

MAGIC = b"SSVID001"
HEADER = struct.Struct("<8s6I")
ENTRY = struct.Struct("<QI")
FPS, MAX_FRAMES, MAX_FRAME, MAX_ARCHIVE = 24, 7200, 2 << 20, 512 << 20
MAX_INPUT = 2 << 30


def sandbox(fd, command, policy):
    args = [sandbox_binary(), "--unshare-user", "--unshare-all", "--disable-userns", "--die-with-parent", "--new-session", "--cap-drop", "ALL", "--clearenv"]
    for key, value in (("PATH", "/usr/bin"), ("LANG", "C"), ("OPENBLAS_NUM_THREADS", "1"), ("OMP_NUM_THREADS", "1")):
        args += ["--setenv", key, value]
    for source in ("/usr", "/lib", "/lib64", "/etc/alternatives", "/etc/ld.so.cache"):
        if Path(source).exists():
            args += ["--ro-bind", source, source]
    return args + ["--proc", "/proc", "--dev", "/dev", "--size", str(64 << 20), "--tmpfs", "/tmp",
                   "--ro-bind-fd", str(fd), "/input.mp4", "--chdir", "/tmp", "--seccomp", str(policy)] + command


def output_chunks(fd, command, seconds):
    """Bound stderr and wall time while streaming stdout; never accumulate an entire clip."""
    with media_process_filter() as policy:
        # Set limits in a trusted native launcher, avoiding preexec_fn in a
        # controller with the parent-lifetime watcher thread already running.
        proc = subprocess.Popen([media_launcher()] + sandbox(fd, command, policy),
                                pass_fds=(fd, policy), stdin=subprocess.DEVNULL, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                start_new_session=True)
    selector = selectors.DefaultSelector()
    selector.register(proc.stdout, selectors.EVENT_READ, "out")
    selector.register(proc.stderr, selectors.EVENT_READ, "err")
    deadline = time.monotonic() + seconds
    errors = bytearray()
    try:
        while selector.get_map():
            if time.monotonic() > deadline:
                raise ValueError("media conversion timed out")
            for key, _ in selector.select(min(0.25, max(0, deadline - time.monotonic()))):
                data = os.read(key.fd, 65536)
                if not data:
                    selector.unregister(key.fileobj)
                elif key.data == "err":
                    errors.extend(data)
                    if len(errors) > 65536:
                        raise ValueError("media tool produced too many errors")
                else:
                    yield data
        if proc.wait(timeout=3) != 0:
            raise ValueError("confined media tool failed: " + errors.decode(errors="replace")[-2000:])
    finally:
        selector.close()
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=3)
        proc.stdout.close()
        proc.stderr.close()


def jpeg_size(data):
    """Read a bounded JPEG marker header without loading a codec into this process."""
    if not data.startswith(b"\xff\xd8") or not data.endswith(b"\xff\xd9"):
        raise ValueError("conversion returned a malformed JPEG frame")
    pos = 2
    while pos + 4 <= len(data):
        if data[pos] != 255:
            raise ValueError("bad JPEG header")
        while pos < len(data) and data[pos] == 255:
            pos += 1
        marker = data[pos]
        pos += 1
        length = int.from_bytes(data[pos:pos+2], "big")
        if length < 2 or pos + length > len(data):
            raise ValueError("bad JPEG segment")
        if marker in (0xC0, 0xC1, 0xC2):
            if length < 8:
                raise ValueError("short JPEG size")
            return int.from_bytes(data[pos+5:pos+7], "big"), int.from_bytes(data[pos+3:pos+5], "big")
        if marker == 0xDA:
            break
        pos += length
    raise ValueError("JPEG has no supported dimensions")


def import_video(source, destination):
    sandbox_binary()
    if not Path("/usr/bin/ffmpeg").is_file() or not Path("/usr/bin/ffprobe").is_file():
        raise ValueError("import needs Omarchy's ffmpeg and ffprobe")
    if os.path.lexists(destination):
        raise FileExistsError("destination already exists")
    source_fd = os.open(source, os.O_RDONLY | os.O_NOFOLLOW | os.O_NONBLOCK)
    temporary = None
    try:
        meta = os.fstat(source_fd)
        if not stat.S_ISREG(meta.st_mode) or not 0 < meta.st_size <= MAX_INPUT:
            raise ValueError("input must be a regular MP4 of at most 2 GiB")
        probe = bytearray()
        for chunk in output_chunks(source_fd, ["/usr/bin/ffprobe", "-v", "error", "-f", "mov", "-protocol_whitelist", "file",
                "-select_streams", "v:0", "-show_entries", "format=duration:stream=width,height", "-of", "json", "/input.mp4"], 15):
            probe.extend(chunk)
            if len(probe) > 65536:
                raise ValueError("media metadata exceeds its limit")
        info = json.loads(probe)
        stream = info["streams"][0]
        duration = float(info["format"]["duration"])
        if not math.isfinite(duration) or not 0 < duration <= 300 or not 0 < stream["width"] * stream["height"] <= 64_000_000:
            raise ValueError("prototype supports clips up to 5 minutes and 64 megapixels")
        destination = Path(destination)
        destination.parent.mkdir(parents=True, exist_ok=True)
        with tempfile.NamedTemporaryFile(prefix=".screensafe-video-", dir=destination.parent, delete=False) as output:
            temporary = Path(output.name)
            output.seek(HEADER.size + MAX_FRAMES * ENTRY.size)
            entries, pending, dimensions = [], bytearray(), None
            command = ["/usr/bin/ffmpeg", "-v", "error", "-nostdin", "-threads", "2", "-filter_threads", "1", "-f", "mov",
                "-protocol_whitelist", "file", "-i", "/input.mp4", "-map", "0:v:0", "-an", "-sn", "-dn", "-map_metadata", "-1",
                "-vf", "scale=w='min(1280,iw)':h='min(720,ih)':force_original_aspect_ratio=decrease:force_divisible_by=2,setsar=1,fps=24",
                "-frames:v", str(MAX_FRAMES+1), "-threads", "2", "-c:v", "mjpeg", "-q:v", "3", "-f", "image2pipe", "pipe:1"]
            for chunk in output_chunks(source_fd, command, 300):
                pending.extend(chunk)
                while (end := pending.find(b"\xff\xd9")) >= 0:
                    frame = bytes(pending[:end+2])
                    del pending[:end+2]
                    if not 0 < len(frame) <= MAX_FRAME or len(entries) >= MAX_FRAMES or output.tell()+len(frame) > MAX_ARCHIVE:
                        raise ValueError("prepared video exceeds frame, duration or storage limits")
                    size = jpeg_size(frame)
                    if not (0 < size[0] <= 1280 and 0 < size[1] <= 720) or (dimensions is not None and size != dimensions):
                        raise ValueError("unexpected or changing frame dimensions")
                    dimensions = size
                    entries.append((output.tell(), len(frame)))
                    output.write(frame)
                if len(pending) > MAX_FRAME:
                    raise ValueError("oversized or unterminated video frame")
            if pending or not entries:
                raise ValueError("video produced no complete frame sequence")
            output.seek(0)
            output.write(HEADER.pack(MAGIC, *dimensions, FPS, len(entries), 0, 0))
            for entry in entries:
                output.write(ENTRY.pack(*entry))
            output.flush()
            os.fsync(output.fileno())
        after = os.fstat(source_fd)
        if (meta.st_size, meta.st_mtime_ns, meta.st_ctime_ns) != (after.st_size, after.st_mtime_ns, after.st_ctime_ns):
            raise ValueError("input changed during conversion")
        # Atomic publication without overwriting an existing file, including a racing symlink.
        os.link(temporary, destination)
        temporary.unlink()
        temporary = None
        return dict(path=str(destination), width=dimensions[0], height=dimensions[1], fps=FPS, frames=len(entries), seconds=len(entries)/FPS,
                    bytes=destination.stat().st_size, source_bytes=meta.st_size)
    finally:
        os.close(source_fd)
        if temporary is not None:
            temporary.unlink(missing_ok=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("source", type=Path)
    parser.add_argument("destination", type=Path)
    args = parser.parse_args()
    try:
        print(json.dumps(import_video(args.source, args.destination)))
    except (ValueError, OSError, KeyError, IndexError, subprocess.SubprocessError) as error:
        parser.exit(1, f"screensafe video import: {error}\n")


if __name__ == "__main__":
    import signal
    def interrupted(*unused):
        raise InterruptedError('video import interrupted')
    signal.signal(signal.SIGTERM, interrupted)
    with ParentLifetime():
        main()
