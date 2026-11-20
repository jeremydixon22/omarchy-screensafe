#!/usr/bin/env python3
"""Bounded local playback records, written by the trusted service, never a pack."""
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import signal
import stat
import sys
import unicodedata

MAX_INPUT = 65536
MAX_RECORD = 16384
MAX_FILE = 1 << 20
NAMES = ('events.jsonl', 'events.1.jsonl', 'events.2.jsonl')
ROOT = Path(__file__).resolve().parent


def text(value, budget):
    value = str(value)[:16384]
    # Diagnostics are local/private, but still omit quoted paths, home paths,
    # command dumps and control characters from their bounded error excerpts.
    value = re.sub(r"(['\"])/[^'\"\r\n]*\1", r'\1<path>\1', value)
    home = os.environ.get('HOME', '')
    if home:
        value = re.sub(re.escape(home) + r'[^\r\n]*', '<home path omitted>', value)
    value = re.sub(r'(?m)(?<![A-Za-z0-9])/(?!/)[^\r\n]*', '<path omitted>', value)
    value = re.sub(r'(?m)^.*(?:Command .*\[|command=|environment=).*$','<command omitted>',value)
    value = ''.join(c for c in value if c in '\n\t' or not unicodedata.category(c).startswith('C'))
    return value.encode('utf-8', errors='replace')[:budget].decode('utf-8', errors='ignore')


def record(data):
    fields = {'event', 'mode', 'pack', 'session', 'durationMs', 'code', 'head', 'tail', 'overflow', 'timeMs', 'route', 'origin'}
    if not isinstance(data, dict) or set(data) != fields:
        raise ValueError('invalid diagnostic fields')
    if data['event'] not in ('start', 'exit') or data['mode'] not in ('fullscreen', 'live-preview'):
        raise ValueError('invalid diagnostic event')
    if data['route'] not in ('renderer','prepared','video-hardware','video-software') or data['origin'] not in ('idle','preview','settings','fallback'):
        raise ValueError('invalid diagnostic route')
    if type(data['timeMs']) is not int or not 0 <= data['timeMs'] <= 2**53:
        raise ValueError('invalid diagnostic time')
    if not isinstance(data['pack'], str) or len(data['pack']) > 4096:
        raise ValueError('invalid diagnostic pack')
    if not isinstance(data['session'], str) or not re.fullmatch(r'[A-Za-z0-9-]{1,100}', data['session']):
        raise ValueError('invalid diagnostic session')
    if any(type(data[k]) is not int or not -255 <= data[k] <= 604800000 for k in ('durationMs', 'code')):
        raise ValueError('invalid diagnostic numbers')
    if not 0 <= data['durationMs'] <= 604800000 or not -255 <= data['code'] <= 255:
        raise ValueError('invalid diagnostic range')
    if type(data['overflow']) is not bool or any(not isinstance(data[k], str) or len(data[k]) > 8192 for k in ('head', 'tail')):
        raise ValueError('invalid diagnostic output')
    pack = Path(data['pack'])
    builtin = pack.parent == ROOT/'packs' and re.fullmatch(r'[a-z0-9-]{1,64}', pack.name)
    # Never open pack data in this trusted writer. Custom paths are identities,
    # not filesystem authority or personal folder names written into the log.
    identity = 'builtin:' + pack.name if builtin else 'custom:' + hashlib.sha256(data['pack'].encode()).hexdigest()[:16]
    result = {k:data[k] for k in ('event', 'mode', 'session', 'durationMs', 'code', 'overflow', 'route', 'origin')}
    result.update(schema=1, time=datetime.datetime.fromtimestamp(data['timeMs']/1000, datetime.timezone.utc).isoformat(timespec='milliseconds'), pack=identity)
    try:
        with (ROOT/'manifest.json').open('rb') as source:
            manifest = json.loads(source.read(4096))
        result['version'] = text(manifest['version'], 80)
    except (OSError, ValueError, KeyError, TypeError):
        result['version'] = 'unknown'
    result['stderrHead'] = text(data['head'], 2048)
    result['stderrTail'] = text(data['tail'], 6144)
    # Extract only known, numeric broker counters and a fixed decoder enum.
    for line in (data['head']+'\n'+data['tail']).splitlines():
        if not line.startswith('{') or len(line) > 2048:
            continue
        try:
            value = json.loads(line)
            decoder = value.get('screensafeVideo', {}).get('decoder')
            if decoder in ('hardware', 'software', 'unavailable'):
                result['decoder'] = decoder
            stats = value.get('ssv2', {})
            counters = {k:v for k,v in stats.items() if k in ('requests','failures','decoderStarts','decodedFrames','cacheHits') and type(v) is int and 0 <= v <= 2**53}
            if counters:
                result['videoCounters'] = counters
        except (ValueError, TypeError, AttributeError):
            continue
    encoded = (json.dumps(result, ensure_ascii=False, separators=(',', ':'))+'\n').encode()
    while len(encoded) > MAX_RECORD:
        result['stderrHead'] = result['stderrHead'][:len(result['stderrHead'])//2]
        result['stderrTail'] = result['stderrTail'][len(result['stderrTail'])//2:]
        result['excerptClipped'] = True
        encoded = (json.dumps(result, ensure_ascii=False, separators=(',', ':'))+'\n').encode()
    return encoded


def private_dir(parent, name):
    try:
        os.mkdir(name, 0o700, dir_fd=parent)
    except FileExistsError:
        pass
    fd = os.open(name, os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC, dir_fd=parent)
    s = os.fstat(fd)
    if s.st_uid != os.getuid() or stat.S_IMODE(s.st_mode) != 0o700:
        os.close(fd)
        raise ValueError('diagnostic directory is not private')
    return fd


def valid_file(s):
    return stat.S_ISREG(s.st_mode) and s.st_uid == os.getuid() and s.st_nlink == 1 and stat.S_IMODE(s.st_mode) == 0o600


def open_file(directory, name):
    fd = os.open(name, os.O_RDWR|os.O_APPEND|os.O_CREAT|os.O_NOFOLLOW|os.O_CLOEXEC|os.O_NONBLOCK, 0o600, dir_fd=directory)
    if not valid_file(os.fstat(fd)):
        os.close(fd)
        raise ValueError('diagnostic file is not private and regular')
    return fd


def append(encoded):
    if not 0 < len(encoded) <= MAX_RECORD:
        raise ValueError('diagnostic record exceeds budget')
    state = os.environ.get('XDG_STATE_HOME', '')
    if not state.startswith('/'):
        home = os.environ.get('HOME', '')
        if not home.startswith('/'):
            raise ValueError('no private state home')
        state = home+'/.local/state'
    Path(state).mkdir(mode=0o700, parents=True, exist_ok=True)
    base = os.open(state, os.O_RDONLY|os.O_DIRECTORY|os.O_NOFOLLOW|os.O_CLOEXEC)
    parent = directory = lock = output = None
    try:
        parent = private_dir(base, 'screensafe')
        directory = private_dir(parent, 'diagnostics')
        lock = open_file(directory, '.lock')
        fcntl.flock(lock, fcntl.LOCK_EX|fcntl.LOCK_NB)
        for name in NAMES:
            try:
                s = os.stat(name, dir_fd=directory, follow_symlinks=False)
            except FileNotFoundError:
                continue
            if not valid_file(s) or s.st_size > MAX_FILE:
                raise ValueError('unsafe or oversized existing diagnostic file')
        output = open_file(directory, NAMES[0])
        size = os.fstat(output).st_size
        if size and os.pread(output, 1, size-1) != b'\n':
            # A shell/process death during a write can leave one partial final
            # record. Repair that bounded suffix before appending another line.
            at = max(0, size-MAX_RECORD)
            tail = os.pread(output, MAX_RECORD, at)
            last = tail.rfind(b'\n')
            if last < 0 and at:
                raise ValueError('invalid diagnostic tail')
            os.ftruncate(output, at+last+1 if last >= 0 else 0)
        if os.fstat(output).st_size + len(encoded) > MAX_FILE:
            os.close(output); output = None
            try:
                os.unlink(NAMES[2], dir_fd=directory)
            except FileNotFoundError:
                pass
            for source, destination in [(NAMES[1],NAMES[2]), (NAMES[0],NAMES[1])]:
                try:
                    os.rename(source, destination, src_dir_fd=directory, dst_dir_fd=directory)
                except FileNotFoundError:
                    pass
            output = open_file(directory, NAMES[0])
        while encoded:
            written = os.write(output, encoded)
            if written <= 0:
                raise OSError('short diagnostic write')
            encoded = encoded[written:]
    finally:
        for fd in (output, lock, directory, parent, base):
            if fd is not None:
                os.close(fd)


def cli():
    def expired(*unused):
        raise TimeoutError('diagnostic deadline exceeded')
    signal.signal(signal.SIGALRM, expired)
    signal.alarm(2)
    try:
        raw = sys.stdin.buffer.readline(MAX_INPUT+1)
        if len(raw) > MAX_INPUT or not raw.endswith(b'\n'):
            raise ValueError('invalid diagnostic input budget')
        append(record(json.loads(raw)))
        print('{"ok":true}')
        return 0
    except (OSError, ValueError, TypeError, OverflowError, RecursionError):
        # Logging failure never becomes a renderer failure or exposes a path.
        print('{"ok":false}')
        return 1
    finally:
        signal.alarm(0)


if __name__ == '__main__':
    raise SystemExit(cli())
