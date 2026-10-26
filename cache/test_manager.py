import contextlib
import errno
import fcntl
import hashlib
import os
from pathlib import Path
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import manager as m


class BoundaryTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)

    def tearDown(self):
        self.temp.cleanup()

    def test_exact_sealed_bytes_survive_source_mutation_and_replacement(self):
        p = self.root/'video'; p.write_bytes(b'approved bytes')
        with m.snapshot(p) as (fd, sha, length):
            p.write_bytes(b'changed'); p.unlink(); p.write_bytes(b'replaced')
            self.assertEqual(sha, hashlib.sha256(b'approved bytes').hexdigest())
            self.assertEqual(os.read(fd, 100), b'approved bytes')
            for mutate in [lambda: os.write(fd, b'x'), lambda: os.ftruncate(fd, 0)]:
                with self.assertRaises(OSError) as e:
                    mutate()
                self.assertEqual(e.exception.errno, errno.EPERM)

    def test_special_symlink_empty_and_oversize_inputs_are_refused(self):
        p = self.root/'video'; p.write_bytes(b'x')
        link = self.root/'link'; link.symlink_to(p)
        fifo = self.root/'fifo'; os.mkfifo(fifo)
        empty = self.root/'empty'; empty.touch()
        huge = self.root/'huge'
        with huge.open('wb') as f:
            f.truncate(m.MAX_VIDEO+1)
        for p in [link, fifo, empty, huge]:
            with self.assertRaises((OSError, ValueError)):
                with m.snapshot(p):
                    self.fail('unsafe input accepted')

    def test_foreign_or_world_readable_approval_directory_is_refused(self):
        p = self.root/'store'; p.mkdir(mode=0o755)
        with self.assertRaises(ValueError):
            with m.store(p):
                self.fail('public trust store accepted')
        p.rmdir(); p.symlink_to(self.root)
        with self.assertRaises(OSError):
            with m.store(p):
                self.fail('linked trust store accepted')

    def test_source_policy_decoder_and_display_changes_invalidate_key(self):
        source = {'fingerprint': 'a'*64}
        profile = {'guard': 'v1', 'size': [2304, 1440], 'decoder': 'software'}
        original = m.cache_key(source, profile)
        self.assertNotEqual(original, m.cache_key({'fingerprint': 'b'*64}, profile))
        for change in [{'guard': 'v2'}, {'size': [1920, 1080]}, {'decoder': 'copy'}]:
            self.assertNotEqual(original, m.cache_key(source, dict(profile, **change)))

    def test_quota_evicts_oldest_and_never_follows_symlinks(self):
        outside = self.root/'outside'; outside.write_bytes(b'keep')
        cache = self.root/'cache'; cache.mkdir()
        old = cache/('a'*64+'.mp4'); old.write_bytes(b'x'*(600 << 10)); os.utime(old, ns=(1, 1))
        newer = cache/('b'*64+'.mp4'); newer.write_bytes(b'x'*(300 << 10)); os.utime(newer, ns=(2, 2))
        linked = cache/('c'*64+'.mp4'); linked.symlink_to(outside)
        unknown = cache/'notes'; unknown.write_bytes(b'keep')
        m.evict(SimpleNamespace(root=cache, limit=1), 500 << 10)
        self.assertFalse(old.exists()); self.assertTrue(newer.exists())
        self.assertEqual(outside.read_bytes(), b'keep'); self.assertTrue(linked.is_symlink())
        self.assertEqual(unknown.read_bytes(), b'keep')

    def test_clear_respects_preparation_lock_and_preserves_unknown_files(self):
        cache = self.root/'cache'
        with m.store(cache) as directory:
            lock = os.open('.lock', os.O_WRONLY | os.O_CREAT, 0o600, dir_fd=directory)
            fcntl.flock(lock, fcntl.LOCK_EX)
            media = cache/('a'*64+'.mp4'); media.write_bytes(b'video')
            with self.assertRaises(BlockingIOError):
                m.clear(SimpleNamespace(root=cache))
            self.assertTrue(media.exists()); os.close(lock)
            unknown = cache/'keep'; unknown.touch()
            m.clear(SimpleNamespace(root=cache))
            self.assertFalse(media.exists()); self.assertTrue(unknown.exists())

    def test_preparation_is_cancelled_on_battery_before_waiting(self):
        p = SimpleNamespace(poll=lambda: None)
        with patch.object(m, 'ac_power', return_value=False):
            with self.assertRaises(ValueError):
                m.wait_preparation([p], float('inf'))


if __name__ == '__main__':
    unittest.main()
