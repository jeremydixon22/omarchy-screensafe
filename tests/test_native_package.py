"""Check the delivered architecture, ABI surface and source/binary receipt."""
import hashlib
import json
from pathlib import Path
import re
import struct
import subprocess
import unittest

ROOT = Path(__file__).resolve().parents[1]


class NativePackageTests(unittest.TestCase):
    def test_receipt_matches_all_six_binaries_and_build_sources(self):
        metadata = json.loads((ROOT/'bin/native-build.json').read_text())
        sha = lambda p: hashlib.sha256(p.read_bytes()).hexdigest()
        self.assertEqual(set(metadata['binaries']), {f'screensafe-{kind}-{arch}' for kind in ['cache','sandbox','media'] for arch in ['x86_64','aarch64']})
        for name, record in metadata['binaries'].items():
            p = ROOT/'bin'/name
            self.assertEqual(sha(p), record['sha256'], name)
            self.assertEqual(p.stat().st_size, record['bytes'], name)
        for field, path in [('inputs_sha256','scripts/native-inputs.json'),
                            ('build_script_sha256','scripts/build-native.py'),
                            ('worker_source_sha256','cache/native/worker.c'),
                            ('media_launcher_source_sha256','cache/native/media-launch.c')]:
            self.assertEqual(metadata[field], sha(ROOT/path), path)

    def test_architectures_and_dependencies_use_only_the_declared_system_abi(self):
        expected = {'cache':{'libmpv.so.2','libEGL.so.1','libGL.so.1','libgbm.so.1','libseccomp.so.2','libc.so.6'},
                    'sandbox':{'libcap.so.2','libc.so.6'}, 'media':set()}
        for arch, machine in [('x86_64','Advanced Micro Devices X86-64'), ('aarch64','AArch64')]:
            for kind, dependencies in expected.items():
                p = ROOT/'bin'/f'screensafe-{kind}-{arch}'
                header = subprocess.check_output(['readelf','-h',str(p)],text=True)
                self.assertIn(machine, header)
                self.assertRegex(header, r'Type:\s+DYN')
                if arch == 'aarch64':
                    data = p.read_bytes()
                    offset = struct.unpack_from('<Q',data,32)[0]
                    stride, count = struct.unpack_from('<HH',data,54)
                    for index in range(count):
                        fields = struct.unpack_from('<IIQQQQQQ',data,offset+index*stride)
                        if fields[0] == 1:  # PT_LOAD: also works on 16/64 KiB kernels.
                            self.assertGreaterEqual(fields[7],65536)
                            self.assertEqual(fields[2] % fields[7],fields[3] % fields[7])
                dynamic = subprocess.check_output(['readelf','-d',str(p)],text=True)
                needed = dependencies | ({'ld-linux-aarch64.so.1'} if arch == 'aarch64' and kind != 'media' else set())
                self.assertEqual(set(re.findall(r'Shared library: \[([^\]]+)\]',dynamic)), needed)
                self.assertNotIn('(RPATH)', dynamic)
                self.assertNotIn('(RUNPATH)', dynamic)
                versions = subprocess.check_output(['readelf','--version-info',str(p)],text=True)
                for major, minor in re.findall(r'GLIBC_(\d+)\.(\d+)',versions):
                    self.assertLessEqual((int(major),int(minor)),(2,34))


if __name__ == '__main__':
    unittest.main()
