#!/usr/bin/env python3
"""Rebuild both native helper sets from pinned compiler, headers and ABI libraries.

Only build inputs are downloaded. Runtime never downloads or builds anything.
The Ubuntu libraries are link-time ABI inputs, not bundled runtime libraries.
Omarchy supplies MPV, graphics drivers, libcap and libseccomp at run time.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import tarfile
import tempfile
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
LOCK = ROOT/'scripts/native-inputs.json'


def digest(path):
    with path.open('rb') as f:
        return hashlib.file_digest(f, 'sha256').hexdigest()


def download(cache, url, expected):
    path = cache/url.rsplit('/', 1)[1]
    if path.exists() and digest(path) == expected:
        return path
    with urllib.request.urlopen(url, timeout=60) as response, tempfile.NamedTemporaryFile(dir=cache, delete=False) as f:
        temporary = Path(f.name)
        try:
            total = 0
            while block := response.read(1 << 20):
                total += len(block)
                if total > 128 << 20:
                    raise ValueError('build input exceeds download limit')
                f.write(block)
            f.flush()
            if digest(temporary) != expected:
                raise ValueError('build input checksum mismatch: '+url)
            os.replace(temporary, path)
        finally:
            temporary.unlink(missing_ok=True)
    return path


def run(command, env):
    subprocess.run([str(x) for x in command], env=env, check=True)


def build(output, cache):
    lock = json.loads(LOCK.read_text())
    host = platform.machine()+'-linux'
    if host not in lock['zig']:
        raise ValueError('build host must be Linux x86_64 or aarch64')
    compiler = lock['zig'][host]
    archive = download(cache, compiler['tarball'], compiler['shasum'])
    upstream = download(cache, lock['bubblewrap']['url'], lock['bubblewrap']['sha256'])
    vendor = ROOT/'vendor'/('bubblewrap-'+lock['bubblewrap']['version'])
    # Upstream license aliases are regular copies for Omarchy's no-symlink
    # installation rule. Verify their resolved bytes as well as all source.
    with tarfile.open(upstream) as source:
        for member in source.getmembers():
            if member.isfile() or member.issym():
                relative = Path(member.name).relative_to(vendor.name)
                if member.issym() and (str(relative) not in {'COPYING.LIB', 'LICENSE'} or member.linkname != 'COPYING'):
                    raise ValueError('unexpected Bubblewrap source symlink')
                if (vendor/relative).is_symlink():
                    raise ValueError('vendored Bubblewrap files must be regular copies')
                if source.extractfile(member).read() != (vendor/relative).read_bytes():
                    raise ValueError('vendored Bubblewrap source differs from pinned release')
    output.mkdir(parents=True, exist_ok=True)
    metadata = {'zig_version': lock['zig_version'], 'glibc_minimum': lock['runtime_abi']['glibc_minimum'],
                'inputs_sha256': digest(LOCK), 'build_script_sha256': digest(Path(__file__)),
                'worker_source_sha256': digest(ROOT/'cache/native/worker.c'),
                'media_launcher_source_sha256': digest(ROOT/'cache/native/media-launch.c'),
                'bubblewrap_archive_sha256': lock['bubblewrap']['sha256'], 'binaries': {}}
    with tempfile.TemporaryDirectory(prefix='screensafe-native-') as directory:
        work = Path(directory)
        with tarfile.open(archive) as source:
            source.extractall(work, filter='data')
        zig = next(work.glob('zig-*/zig'))
        env = {'PATH': '/usr/bin:/bin', 'HOME': str(work), 'LC_ALL': 'C', 'TZ': 'UTC',
               'ZIG_GLOBAL_CACHE_DIR': str(work/'zig-global'), 'ZIG_LOCAL_CACHE_DIR': str(work/'zig-local'),
               'SOURCE_DATE_EPOCH': '1700000000'}
        for arch, deb_arch, multi in [('x86_64', 'amd64', 'x86_64-linux-gnu'),
                                      ('aarch64', 'arm64', 'aarch64-linux-gnu')]:
            sysroot = work/deb_arch
            sysroot.mkdir()
            for entry in lock['packages'][deb_arch]:
                package = download(cache, entry['url'], entry['sha256'])
                run(['/usr/bin/dpkg-deb', '-x', package, sysroot], env)
            config = work/('config-'+arch)
            config.mkdir()
            (config/'config.h').write_text('#define PACKAGE_STRING "bubblewrap '+lock['bubblewrap']['version']+'"\n')
            remap = '-ffile-prefix-map='+str(ROOT)+'=/screensafe'
            flags = [zig, 'cc', '-target', arch+'-linux-gnu.'+lock['runtime_abi']['glibc_minimum'],
                     '-mcpu=baseline', '-O2', '-fPIE', '-pie', '-fstack-protector-strong', '-D_FORTIFY_SOURCE=3',
                     '-Wl,-z,relro,-z,now,--build-id=none', '-s', '-fno-ident', remap,
                     '-ffile-prefix-map='+str(work)+'=/build',
                     '-I', sysroot/'usr/include', '-L', sysroot/'usr/lib'/multi,
                     '-Wl,--allow-shlib-undefined']
            worker = output/('screensafe-cache-'+arch)
            run(flags+['-Wall', '-Wextra', '-Werror', '-Wno-misleading-indentation',
                       ROOT/'cache/native/worker.c', '-o', worker,
                       '-lmpv', '-lEGL', '-lGL', '-lgbm', '-lseccomp'], env)
            sandbox = output/('screensafe-sandbox-'+arch)
            sources = [vendor/name for name in ['bubblewrap.c', 'bind-mount.c', 'network.c',
                                                'utils.c', 'chroot_realpath.c', 'safe_openat.c']]
            run(flags+['-D_GNU_SOURCE', '-I', config, '-I', vendor]+sources+['-o', sandbox, '-lcap'], env)
            launcher = output/('screensafe-media-'+arch)
            run([zig, 'cc', '-target', arch+'-linux-musl', '-mcpu=baseline', '-O2', '-static', '-fPIE', '-pie', '-s',
                 '-fno-ident', remap, '-ffile-prefix-map='+str(work)+'=/build',
                 '-Wall', '-Wextra', '-Werror', '-Wl,--build-id=none',
                 ROOT/'cache/native/media-launch.c', '-o', launcher], env)
            for binary in [worker, sandbox, launcher]:
                binary.chmod(0o755)
                metadata['binaries'][binary.name] = {'sha256': digest(binary), 'bytes': binary.stat().st_size}
                print(metadata['binaries'][binary.name]['sha256'], binary.name, flush=True)
    (output/'native-build.json').write_text(json.dumps(metadata, sort_keys=True, indent=2)+'\n')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('output', type=Path)
    parser.add_argument('--cache', type=Path, default=Path(os.environ.get('XDG_CACHE_HOME', str(Path.home()/'.cache')))/'screensafe-build')
    args = parser.parse_args()
    args.cache.mkdir(parents=True, exist_ok=True)
    build(args.output.resolve(), args.cache.resolve())


if __name__ == '__main__':
    main()
