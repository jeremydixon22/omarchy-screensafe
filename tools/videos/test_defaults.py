"""Default-library selection: no codec processes or display needed."""
from pathlib import Path
import os
import tempfile
import unittest
from unittest.mock import patch

import play_v2


class DefaultVideoTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.examples = self.root / 'examples'
        self.examples.mkdir()
        (self.examples / 'omarchy-ad-loop.ssv').write_bytes(b'metadata-only selector fixture')
        self.personal = self.root / 'personal'
        self.personal.mkdir()
        self.command = ['render', '/pack', '--videos', str(self.personal)]
        patcher = patch.object(play_v2, 'DEFAULT_VIDEOS', self.examples)
        patcher.start()
        self.addCleanup(patcher.stop)

    def test_empty_missing_and_renamed_video_libraries_use_examples(self):
        for name in ['Roadster.ssx', 'source.mp4', '.hidden.ssv', 'bad\nname.ssv']:
            (self.personal / name).write_bytes(b'untouched')
        self.assertEqual(play_v2.default_video_source(self.command)[-1], str(self.examples))
        self.assertEqual(self.command[-1], str(self.personal))
        missing = ['render', '/pack', '--videos', str(self.root / 'missing')]
        self.assertEqual(play_v2.default_video_source(missing)[-1], str(self.examples))
        self.assertEqual((self.personal / 'Roadster.ssx').read_bytes(), b'untouched')

    def test_regular_personal_video_takes_precedence_and_is_not_opened(self):
        (self.personal / 'Yours.SSV').write_bytes(b'not decoded by this selector')
        self.assertEqual(play_v2.default_video_source(self.command), self.command)

    def test_symlinks_directories_and_fifos_do_not_suppress_examples(self):
        (self.personal / 'link.ssv').symlink_to(self.examples / 'omarchy-ad-loop.ssv')
        (self.personal / 'directory.ssv').mkdir()
        os.mkfifo(self.personal / 'pipe.ssv')
        self.assertEqual(play_v2.default_video_source(self.command)[-1], str(self.examples))

    def test_uncertain_and_over_budget_libraries_retain_the_requested_directory(self):
        with patch.object(play_v2.os, 'scandir', side_effect=PermissionError):
            self.assertEqual(play_v2.default_video_source(self.command), self.command)
        for i in range(3):
            (self.personal / str(i)).touch()
        with patch.object(play_v2, 'LIBRARY_SCAN_LIMIT', 2):
            self.assertEqual(play_v2.default_video_source(self.command), self.command)

    def test_missing_or_linked_default_does_not_change_the_request(self):
        example = self.examples / 'omarchy-ad-loop.ssv'
        example.unlink()
        self.assertEqual(play_v2.default_video_source(self.command), self.command)
        example.symlink_to(self.root / 'absent')
        self.assertEqual(play_v2.default_video_source(self.command), self.command)

    def test_implicit_home_and_repeated_options_match_renderer_argument_rules(self):
        with patch.dict(os.environ, {'HOME': str(self.root)}):
            self.assertEqual(play_v2.default_video_source(['catalog', '/packs'])[-2:], ['--videos', str(self.examples)])
        (self.personal / 'mine.ssv').touch()
        repeated = ['check', '/pack', '--videos', str(self.root / 'missing'), '--videos', str(self.personal)]
        self.assertEqual(play_v2.default_video_source(repeated), repeated)
        incomplete = ['render', '/pack', '--videos']
        self.assertEqual(play_v2.default_video_source(incomplete), incomplete)
        self.assertEqual(play_v2.default_video_source(['videos-folder']), ['videos-folder'])


if __name__ == '__main__':
    unittest.main()
