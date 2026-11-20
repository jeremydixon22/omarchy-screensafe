"""Exercise private file authority, budgets and the real one-event writer CLI."""
from concurrent.futures import ThreadPoolExecutor
import fcntl
import json
import os
from pathlib import Path
import stat
import subprocess
import sys
import tempfile
import time
import unittest
from unittest.mock import patch

import screensafe_diagnostics as d

ROOT = Path(__file__).resolve().parents[1]


def event(**changes):
    return dict(event='exit', mode='fullscreen', pack=str(ROOT/'packs/videos'), session='s-test-1',
                durationMs=123, code=3, head='', tail='', overflow=False,
                timeMs=time.time_ns()//1000000, route='video-hardware', origin='idle') | changes


class DiagnosticsTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix='screensafe-diagnostics-')
        self.base = Path(self.temp.name)
        self.env = dict(os.environ, HOME=str(self.base/'home'), XDG_STATE_HOME=str(self.base/'state'))
        self.patch = patch.dict(os.environ, self.env)
        self.patch.start()
        self.logs = self.base/'state/screensafe/diagnostics'

    def tearDown(self):
        self.patch.stop()
        self.temp.cleanup()

    def cli(self, value):
        raw = value if isinstance(value, bytes) else (json.dumps(value)+'\n').encode()
        return subprocess.run([sys.executable, '-B', str(ROOT/'screensafe_diagnostics.py')],
                              input=raw, capture_output=True, timeout=4, env=self.env)

    def test_actual_cli_writes_private_records_and_extracts_only_known_counters(self):
        output = 'screensafe: video frame 41 failed: SSVv2 pixel stream ended early\n'
        output += '{"screensafeVideo":{"decoder":"hardware"}}\n'
        output += '{"ssv2":{"requests":42,"failures":0,"decodedFrames":41,"unknown":"secret"}}\n'
        p = self.cli(event(head=output, tail=output))
        self.assertEqual(p.returncode, 0, p.stderr)
        self.assertEqual(json.loads(p.stdout), {'ok':True})
        r = json.loads((self.logs/'events.jsonl').read_text())
        self.assertEqual(r['pack'], 'builtin:videos')
        self.assertEqual(r['decoder'], 'hardware')
        self.assertEqual(r['videoCounters'], {'requests':42,'failures':0,'decodedFrames':41})
        self.assertIn('pixel stream ended early', r['stderrHead'])
        self.assertEqual(stat.S_IMODE(self.logs.stat().st_mode), 0o700)
        for p in self.logs.iterdir():
            self.assertEqual(stat.S_IMODE(p.stat().st_mode), 0o600)
        self.assertNotIn('environment', r)

    def test_rotations_keep_three_bounded_complete_jsonl_files(self):
        with patch.object(d, 'MAX_FILE', 1024):
            for i in range(30):
                d.append(d.record(event(session=f's-{i}', tail='x'*200)))
            files = [p for p in self.logs.iterdir() if p.suffix == '.jsonl']
            self.assertEqual(len(files), 3)
            self.assertLessEqual(sum(p.stat().st_size for p in files), 3*d.MAX_FILE)
            records = [json.loads(line) for p in files for line in p.read_text().splitlines()]
            self.assertIn('s-29', [r['session'] for r in records])
            self.assertNotIn('s-0', [r['session'] for r in records])

    def test_log_paths_symlinks_hardlinks_and_nonprivate_files_are_refused(self):
        victim = self.base/'victim'
        victim.write_bytes(b'keep'); victim.chmod(0o600)
        for name, attack in [('events.jsonl','symlink'), ('events.1.jsonl','symlink'), ('.lock','symlink'),
                             ('events.jsonl','hardlink'), ('events.jsonl','mode')]:
            with self.subTest(name=name, attack=attack):
                d.append(d.record(event()))
                path = self.logs/name
                path.unlink(missing_ok=True)
                if attack == 'symlink':path.symlink_to(victim)
                elif attack == 'hardlink':os.link(victim, path)
                else:path.write_bytes(b'not private');path.chmod(0o644)
                p = self.cli(event())
                self.assertEqual(p.returncode, 1)
                self.assertEqual(victim.read_bytes(), b'keep')
                path.unlink()

    def test_partial_last_record_is_repaired_without_losing_complete_history(self):
        d.append(d.record(event(session='s-first')))
        with (self.logs/'events.jsonl').open('ab') as output:
            output.write(b'{"partial')
        d.append(d.record(event(session='s-second')))
        records = [json.loads(line) for line in (self.logs/'events.jsonl').read_text().splitlines()]
        self.assertEqual([r['session'] for r in records], ['s-first','s-second'])

    def test_directory_symlink_does_not_create_logs_at_target(self):
        target = self.base/'target';target.mkdir()
        parent = self.base/'state/screensafe';parent.mkdir(parents=True, mode=0o700)
        (parent/'diagnostics').symlink_to(target, target_is_directory=True)
        self.assertEqual(self.cli(event()).returncode, 1)
        self.assertEqual(list(target.iterdir()), [])

    def test_lock_contention_drops_record_promptly_without_waiting(self):
        d.append(d.record(event()))
        with (self.logs/'.lock').open('rb') as lock:
            fcntl.flock(lock, fcntl.LOCK_EX)
            t = time.monotonic(); p = self.cli(event())
            self.assertEqual(p.returncode, 1)
            self.assertLess(time.monotonic()-t, 1.5)
        self.assertEqual(len((self.logs/'events.jsonl').read_text().splitlines()), 1)

    def test_concurrent_cli_writes_never_mix_records(self):
        with ThreadPoolExecutor(max_workers=4) as pool:
            results = list(pool.map(lambda i:self.cli(event(session=f's-{i}')), range(12)))
        accepted = sum(p.returncode == 0 for p in results)
        self.assertGreater(accepted, 0)
        self.assertTrue(all(p.returncode in (0,1) for p in results))
        lines = (self.logs/'events.jsonl').read_text().splitlines()
        self.assertEqual(len(lines), accepted)
        self.assertEqual(len({json.loads(line)['session'] for line in lines}), accepted)

    def test_bad_schema_oversized_and_truncated_input_never_write(self):
        for value in [b'{"unfinished":', b'x'*65537+b'\n', event(destination=str(self.base/'escape')),
                      event(code=True), event(mode='arbitrary'), event(head=9), event(session='bad\nline')]:
            with self.subTest(value=str(value)[:60]):
                p = self.cli(value)
                self.assertEqual(p.returncode, 1)
                self.assertEqual(p.stderr, b'')
                self.assertEqual(json.loads(p.stdout), {'ok':False})
                self.assertFalse(self.logs.exists())

    def test_incomplete_open_input_has_an_independent_deadline(self):
        p = subprocess.Popen([sys.executable, '-B', str(ROOT/'screensafe_diagnostics.py')],
                             stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.PIPE, env=self.env)
        try:
            self.assertEqual(p.wait(timeout=3.5), 1)
            self.assertEqual(json.loads(p.stdout.read()), {'ok':False})
        finally:
            if p.poll() is None:p.kill();p.wait()
            p.stdin.close();p.stdout.close();p.stderr.close()

    def test_custom_pack_paths_and_command_dumps_are_not_persisted(self):
        secret = self.env['HOME']+'/Pictures/My private holiday/photo.jpg'
        r = json.loads(d.record(event(pack=secret, head=f'cannot read "{secret}"\nCommand [\'secret argument\']\n\x1b[31m', tail='')))
        self.assertTrue(r['pack'].startswith('custom:'))
        self.assertNotIn('private holiday', json.dumps(r))
        self.assertNotIn('secret argument', json.dumps(r))
        self.assertNotIn('\x1b', r['stderrHead'])

    def test_hostile_excerpt_remains_one_bounded_json_record(self):
        raw = d.record(event(head='"\n'*4000, tail='☃'*8192))
        self.assertLessEqual(len(raw), d.MAX_RECORD)
        self.assertEqual(raw.count(b'\n'), 1)
        self.assertEqual(json.loads(raw)['event'], 'exit')


if __name__ == '__main__':
    unittest.main()
