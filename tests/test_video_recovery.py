"""Exercise the actual broker's one-shot recovery state and descriptor cleanup."""
import array
import contextlib
import io
import json
import os
from pathlib import Path
import socket
import sys
import tempfile
from types import SimpleNamespace
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]/'tools/videos'))
import play_v2


class VideoRecoveryTests(unittest.TestCase):
    def exercise(self, requests, hardware=True, delivery='pipe', close_delay=0, cancel=False, recovery_delay=0, stuck=False):
        """Use real received-FD ownership, but no codecs/devices in unit tests."""
        with tempfile.TemporaryDirectory() as directory:
            files=[]
            for name in ['first','other']:
                p=Path(directory)/name;p.write_bytes(name.encode());files.append(os.open(p,os.O_RDONLY))
            received=[];replies=[];calls=[];clock=[100.0]
            authority=object() if hardware else None
            queue=list(requests)
            class Channel:
                def recvmsg(self,*unused):
                    if not queue:return b'',[],0,None
                    target,budget,identity=queue.pop(0)
                    if budget:clock[0]+=recovery_delay
                    fd=os.dup(files[identity]);received.append(fd)
                    return play_v2.REQUEST.pack(play_v2.MAGIC,len(received),target,budget),[(socket.SOL_SOCKET,socket.SCM_RIGHTS,array.array('i',[fd]).tobytes())],0,None
                def sendmsg(self,data,ancillary):
                    replies.append(play_v2.RESPONSE.unpack(data[0]));return len(data[0])
                def send(self,data):replies.append(play_v2.RESPONSE.unpack(data));return len(data)
            class Frames:
                info={'width':2,'height':2}
                starts=decodes=hits=delivery=0
                def __init__(self,actual):
                    if actual is not authority:raise AssertionError('hardware authority changed')
                    self.closes=0
                    self.decoder=SimpleNamespace(proc=SimpleNamespace(poll=lambda:None if stuck else 0))
                def frame(self,fd,target,deadline=None):
                    calls.append(('frame',target,deadline))
                    self.starts+=1
                    self.delivery=0 if deadline is not None else 1
                    return files[0]
                def close(self):
                    self.closes+=1;calls.append(('close',))
                    clock[0]+=close_delay
                    if cancel and self.closes==1:raise InterruptedError('cancelled during recovery cleanup')
            error=None
            try:
                with patch.object(play_v2,'StreamFrames',Frames),patch.object(play_v2,'Frames',Frames),patch.object(play_v2.time,'monotonic',lambda:clock[0]),contextlib.redirect_stderr(io.StringIO()) as log:
                    try:play_v2.serve(Channel(),None,delivery,authority)
                    except (ValueError,InterruptedError) as exc:error=exc
                for fd in received:
                    with self.assertRaises(OSError):os.fstat(fd)
                stats=[json.loads(line)['ssv2'] for line in log.getvalue().splitlines()if line.startswith('{"ssv2":')][-1]
                return {'error':error,'calls':calls,'replies':replies,'stats':stats,'stderr':log.getvalue()}
            finally:
                for fd in files:os.close(fd)

    def test_same_frame_recovery_closes_old_decoder_and_returns_a_complete_snapshot(self):
        r=self.exercise([(7,0,0),(7,play_v2.RECOVERY|1200,0)])
        self.assertIsNone(r['error'])
        self.assertEqual([x[0]for x in r['calls']],['frame','close','frame','close'])
        retry=r['calls'][2]
        self.assertEqual(retry[1],7);self.assertAlmostEqual(retry[2],101.2)
        self.assertEqual([x[7]for x in r['replies']],[1,0])
        self.assertEqual(r['stats']['recoveryAttempts'],1)
        self.assertEqual(r['stats']['decoderStarts'],2)

    def test_recovery_authority_identity_budget_and_immediate_predecessor_are_required(self):
        cases=[
            ([(7,play_v2.RECOVERY|1000,0)],True,'pipe'),
            ([(7,0,0),(7,play_v2.RECOVERY|1000,0)],False,'pipe'),
            ([(7,0,0),(7,play_v2.RECOVERY|1000,0)],True,'snapshot'),
            ([(7,0,0),(8,play_v2.RECOVERY|1000,0)],True,'pipe'),
            ([(7,0,0),(7,play_v2.RECOVERY|1000,1)],True,'pipe'),
            ([(7,0,0),(7,1,0)],True,'pipe'),
            ([(7,0,0),(7,play_v2.RECOVERY,0)],True,'pipe'),
            ([(7,0,0),(7,play_v2.RECOVERY|5001,0)],True,'pipe'),
            ([(7,0,0),(7,0xC0000001,0)],True,'pipe'),
        ]
        for requests,hardware,delivery in cases:
            with self.subTest(requests=requests,hardware=hardware,delivery=delivery):
                r=self.exercise(requests,hardware,delivery)
                self.assertIsInstance(r['error'],ValueError)
                self.assertEqual(r['stats']['recoveryAttempts'],0)
                self.assertEqual(sum(x[0]=='frame'for x in r['calls']),len(requests)-1)

    def test_a_second_recovery_is_refused_even_after_another_normal_stream(self):
        r=self.exercise([(7,0,0),(7,play_v2.RECOVERY|1000,0),(8,0,0),(8,play_v2.RECOVERY|1000,0)])
        self.assertIsInstance(r['error'],ValueError)
        self.assertEqual(r['stats']['recoveryAttempts'],1)
        self.assertEqual(r['stats']['decoderStarts'],3)

    def test_recovery_cleanup_consumes_remaining_budget_before_a_new_decoder_can_start(self):
        r=self.exercise([(7,0,0),(7,play_v2.RECOVERY|1,0)],close_delay=.002)
        self.assertIsNone(r['error'])
        self.assertEqual(r['stats']['decoderStarts'],1)
        self.assertEqual(r['stats']['failures'],1)
        self.assertEqual(r['replies'][-1][6],1)
        self.assertIn('recovery deadline exceeded',r['stderr'])

    def test_cancellation_during_recovery_never_starts_replacement_or_sends_a_refusal(self):
        r=self.exercise([(7,0,0),(7,play_v2.RECOVERY|1000,0)],cancel=True)
        self.assertIsInstance(r['error'],InterruptedError)
        self.assertEqual(r['stats']['decoderStarts'],1)
        self.assertEqual(len(r['replies']),1)

    def test_client_cannot_extend_the_original_broker_deadline_by_delaying_a_retry(self):
        r=self.exercise([(7,0,0),(7,play_v2.RECOVERY|5000,0)],recovery_delay=6)
        self.assertIsNone(r['error'])
        self.assertEqual(r['stats']['decoderStarts'],1)
        self.assertEqual(r['stats']['failures'],1)
        self.assertEqual(r['replies'][-1][6],1)

    def test_a_decoder_that_survives_bounded_cleanup_prevents_any_replacement(self):
        r=self.exercise([(7,0,0),(7,play_v2.RECOVERY|5000,0)],stuck=True)
        self.assertIsNone(r['error'])
        self.assertEqual(r['stats']['decoderStarts'],1)
        self.assertEqual(r['stats']['failures'],1)
        self.assertIn('previous decoder did not stop; recovery refused',r['stderr'])
        self.assertEqual(r['replies'][-1][6],1)


if __name__=='__main__':unittest.main()
