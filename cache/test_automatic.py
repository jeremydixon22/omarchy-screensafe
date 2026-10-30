"""Progress hints cannot grant approval, and managed cancellation drains work."""
import io
import json
import os
from pathlib import Path
import tempfile
import threading
import time
from types import SimpleNamespace
import unittest
from unittest.mock import patch

import automatic
import manager
from progress import Display, Reader


class ProgressTests(unittest.TestCase):
    def test_codec_frame_counts_are_bounded_to_their_stage(self):
        values=[]
        reader=Reader(io.BytesIO(b'frame=0\nprogress=continue\nframe=8\nprogress=end\n'),lambda *v:values.append(v),total=8,stage='recording',start=.02,end=.4,ffmpeg=True)
        reader.finish();self.assertEqual(values[-1],(.4,'recording'))
        for data in [b'frame=9\n',b'frame=2\nframe=1\n',b'frame=NaN\n',b'x'*513]:
            with self.subTest(data=data[:20]):
                reader=Reader(io.BytesIO(data),lambda *_:None,total=8,stage='recording',start=0,end=.4,ffmpeg=True)
                with self.assertRaises(ValueError):reader.finish()
    def test_full_verification_progress_remains_advisory(self):
        values=[]
        reader=Reader(io.BytesIO(b'{"verifiedFrames":16,"totalFrames":16}\n'),lambda *v:values.append(v),total=16,stage='software',start=.4,end=.65)
        reader.finish();self.assertEqual(values,[(.65,'software')])
        reader=Reader(io.BytesIO(b'{"approved":true,"state":"ready"}\n'),lambda *_:self.fail('codec authorized readiness'),total=16,stage='software',start=.4,end=.65)
        with self.assertRaises(ValueError):reader.finish()
    def test_a_nonreading_viewer_cannot_block_progress_or_authorize_early_ready(self):
        read,write=os.pipe()
        try:
            with os.fdopen(write,'wb',buffering=0) as stream:
                display=Display(stream);began=time.monotonic()
                for _ in range(2000):display.last=0;display(1,'finishing')
                self.assertLess(time.monotonic()-began,.5)
                self.assertEqual(display.value,999)
        finally:os.close(read)


class PowerTests(unittest.TestCase):
    def test_full_battery_does_not_override_an_offline_external_supply(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);battery=root/'BAT';battery.mkdir();(battery/'type').write_text('Battery');(battery/'status').write_text('Full')
            mains=root/'AC';mains.mkdir();(mains/'type').write_text('Mains');(mains/'online').write_text('0')
            real_path=Path
            with patch.object(manager,'Path',side_effect=lambda p:root if p=='/sys/class/power_supply' else real_path(p)):
                self.assertFalse(manager.ac_power())
                (mains/'online').write_text('1');self.assertTrue(manager.ac_power())
                (mains/'online').unlink();(mains/'type').unlink();mains.rmdir();self.assertFalse(manager.ac_power())
                (battery/'status').write_text('Charging');self.assertTrue(manager.ac_power())
    def test_battery_only_reuse_still_allows_ac_generation_and_battery_misses_stay_live(self):
        with tempfile.TemporaryDirectory() as tmp:
            args=SimpleNamespace(root=Path(tmp)/'cache',battery_only=1,overlay='enabled=0',automatic=1)
            with patch.object(manager,'ac_power',return_value=True),patch.object(manager,'on_battery',return_value=False),patch.object(automatic,'run',return_value=0) as run:
                self.assertEqual(manager.run(args),0);run.assert_called_once_with(args,manager)
            class Live(Exception):pass
            with patch.object(manager,'ac_power',return_value=False),patch.object(manager,'on_battery',return_value=True), \
                    patch.object(manager,'available',return_value=True),patch.object(manager,'live',side_effect=Live), \
                    patch.object(automatic,'run',side_effect=AssertionError('battery started preparation')):
                with self.assertRaises(Live):manager.run(args)
            self.assertEqual(list(args.root.iterdir()),[])


class LifecycleTests(unittest.TestCase):
    def test_managed_control_eof_cancels_preparation_and_reaps_the_viewer(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp);renderer=root/'viewer';marker=root/'viewer-started'
            renderer.write_text('#!/usr/bin/python3\nimport json,sys,time\nfrom pathlib import Path\nPath('+repr(str(marker))+').write_text("started")\nprint(json.dumps({"screensafePreparation":{"mode":"overlay"}}),flush=True)\nfor line in sys.stdin:\n if line.strip()=="quit":break\n')
            renderer.chmod(0o755);worker=root/'worker';worker.touch();cache=root/'cache'
            args=SimpleNamespace(renderer=renderer,worker=worker,root=cache,overlay='enabled=0',role='org.omarchy.screensaver',exit_after=None,managed=True,pack=str(root),battery_only=0)
            began=threading.Event();cancelled=threading.Event()
            def prepare(job_args,progress):
                began.set()
                while not job_args.cancel_event.wait(.01):pass
                cancelled.set();raise ValueError('cancelled')
            read,write=os.pipe();control=os.fdopen(read,'rb',buffering=0)
            def close():
                self.assertTrue(began.wait(3));os.close(write)
            closer=threading.Thread(target=close)
            with patch.object(manager,'STOP',False),patch.object(manager,'available',return_value=True), \
                    patch.object(manager,'plan',return_value={'eligible':True,'fingerprint':'a'*64}), \
                    patch.object(manager,'geometry',return_value=[320,200]),patch.object(manager,'status',return_value={'state':'empty'}), \
                    patch.object(manager,'prepare',side_effect=prepare),patch.object(manager,'ac_power',return_value=True), \
                    patch('sys.stdin',control):
                closer.start();code=automatic.run(args,manager)
            closer.join(timeout=3);control.close();self.assertEqual(code,0);self.assertTrue(cancelled.is_set());self.assertTrue(marker.exists())
    def test_status_rejects_a_matching_record_with_changed_or_missing_media(self):
        with tempfile.TemporaryDirectory() as tmp:
            root=Path(tmp)/'cache';root.mkdir(mode=0o700);key='a'*64
            (root/(key+'.json')).write_text('{}');(root/(key+'.mp4')).write_bytes(b'changed')
            args=SimpleNamespace(root=root)
            record={'sha256':'0'*64,'bytes':7}
            with patch.object(manager,'available',return_value=True),patch.object(manager,'plan',return_value={'eligible':True}), \
                    patch.object(manager,'geometry',return_value=[320,200]),patch.object(manager,'profile',return_value={}), \
                    patch.object(manager,'lookup',return_value=(key,record,None,None)):
                self.assertEqual(manager.status(args)['state'],'empty')
                (root/(key+'.mp4')).unlink()
                self.assertEqual(manager.status(args)['state'],'empty')

    def test_retry_records_are_private_do_not_follow_symlinks_and_expire(self):
        with tempfile.TemporaryDirectory() as tmp:
            path=Path(tmp)/'failure';automatic.record_failure(path);self.assertTrue(automatic.recently_failed(path))
            self.assertEqual(path.stat().st_mode&0o777,0o600)
            path.write_text(json.dumps({'failedAt':time.time()-601}));self.assertFalse(automatic.recently_failed(path))
            automatic.record_failure(path);self.assertTrue(automatic.recently_failed(path))
            path.unlink();outside=Path(tmp)/'outside';outside.write_text('keep');path.symlink_to(outside)
            self.assertFalse(automatic.recently_failed(path))
            with self.assertRaises(OSError):automatic.record_failure(path)
            self.assertEqual(outside.read_text(),'keep')
            path.unlink();outside.chmod(0o600);os.link(outside,path)
            with self.assertRaises(ValueError):automatic.record_failure(path)
            self.assertEqual(outside.read_text(),'keep')
