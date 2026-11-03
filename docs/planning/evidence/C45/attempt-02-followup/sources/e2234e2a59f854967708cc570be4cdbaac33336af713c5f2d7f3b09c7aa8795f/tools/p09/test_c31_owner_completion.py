"""Synthetic temporary-file policy tests; never creates a repository verdict."""
from pathlib import Path
import hashlib
import json
import sys
import tempfile
import unittest
from unittest.mock import patch
sys.path.insert(0, str(Path(__file__).resolve().parent))
import c31_owner_completion as policy
import c31_transition_policy as transition
import phase_inputs_c31_transition as inputs


class ConditionalCompletion(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.directory = self.root / 'docs/planning/reviews/C31/attempt-07'
        self.directory.mkdir(parents=True)
        self.note = self.root / policy.DECISION
        self.note.parent.mkdir(parents=True, exist_ok=True)
        self.note.write_text('SYNTHETIC TEST OWNER NOTE')
        self.addCleanup(patch.stopall)
        patch.object(policy, 'DECISION_SHA', policy.original.sha(self.note)).start()
        self.transition = {'record': 'transition.json', 'sha256': hashlib.sha256(b'{}').hexdigest()}
        (self.root / 'transition.json').write_text('{}')
        (self.root / 'checks.json').write_text('{}')
        (self.root / 'source.txt').write_text('SYNTHETIC SOURCE')
        files = {policy.DECISION: policy.original.sha(self.note), 'source.txt': policy.original.sha(self.root / 'source.txt')}
        source = hashlib.sha256(''.join(f'{p} {h}\n' for p,h in sorted(files.items())).encode()).hexdigest()
        self.scores = dict(architecture=9, behavior=8, authority_security=8, validation=8, overall=8)
        self.verdict = dict(subject='C31', attempt_id='attempt-07', request_id='synthetic-only',
            fingerprints=dict(source=source, dependencies='d', build_flags='b', evidence='e'),
            scores=self.scores, recommendation='fail', required_fixes=['synthetic fix'],
            blockers=['synthetic blocker retained; owner added no no-blocker rule'], evidence_gaps=['synthetic gap'])
        self.request = {k:self.verdict[k] for k in ['subject','attempt_id','request_id','fingerprints']}
        self.request.update(scope={'files':sorted(files)}, source_transition=self.transition,
            commands=[{'record':'checks.json'}], c31_final_review_policy={
                'decision':policy.DECISION, 'decision_sha256':policy.DECISION_SHA,
                'last_completed_at_direction':'attempt-06'})
        self.write('verdict-returned.json',self.verdict)
        self.write('request.json',self.request)
        self.write('source-inventory.json',{'files':files,'source_fingerprint':source})
        self.write('invocation.json',{'synthetic':True})
        self.write('thread-reply.log',{'synthetic':True})
        self.audit=dict(raw_verdict_sha256=self.sha('verdict-returned.json'),request_sha256=self.sha('request.json'),
            scores=self.scores,full_gate_pass=False,compiled_inputs_unchanged=True,transport={'synthetic':True})
        self.write('post-review-audit.json',self.audit)
        self.receipt={k:self.verdict[k] for k in ['scores','recommendation','fingerprints','required_fixes','blockers','evidence_gaps']}
        self.receipt.update(verdict_sha256=self.sha('verdict-returned.json'),request_sha256=self.sha('request.json'),
            post_review_audit_sha256=self.sha('post-review-audit.json'),full_gate_pass=False)
        self.write('receipt.json',self.receipt)
        patch.object(policy.original,'verify_launch',return_value={'synthetic':True}).start()
        patch.object(transition,'verify_transition',return_value=self.transition).start()
        patch.object(inputs,'verify_native',return_value={'build_inputs':{'input_digest':'d'},'records':[]}).start()
        patch.object(inputs,'verify_checks',return_value=None).start()

    def write(self,name,value):
        (self.directory/name).write_text(json.dumps(value))
    def sha(self,name):return policy.original.sha(self.directory/name)

    def test_low_scores_even_with_blocker_activate_owner_without_changing_raw_results(self):
        raw={name:self.sha(name) for name in ['verdict-returned.json','post-review-audit.json','receipt.json']}
        value=policy.activate(self.root)
        self.assertTrue(value['owner_closed'])
        self.assertTrue(value['owner_complete'])
        self.assertFalse(value['full_gate_pass'])
        self.assertEqual(set(value['owner_scores'].values()),{9})
        self.assertEqual(value['scores'],self.scores)
        self.assertEqual(value['accepted_findings']['blockers'],self.verdict['blockers'])
        self.assertEqual(raw,{name:self.sha(name) for name in raw})

    def test_attempt06_cannot_activate(self):
        self.directory.rename(self.directory.with_name('attempt-06'))
        with self.assertRaisesRegex(ValueError,'no completed actual'):
            policy.activate(self.root)

    def test_no_verdict_transport_failure_cannot_activate(self):
        (self.directory/'verdict-returned.json').unlink()
        with self.assertRaisesRegex(ValueError,'transport failure is not a score'):
            policy.activate(self.root)

    def test_actual_nines_do_not_manufacture_owner_activation(self):
        self.verdict['scores']={k:9 for k in self.scores}
        self.write('verdict-returned.json',self.verdict)
        with self.assertRaisesRegex(ValueError,'condition not triggered'):
            policy.activate(self.root)

    def test_changed_source_is_rejected(self):
        (self.root/'source.txt').write_text('changed')
        with self.assertRaisesRegex(ValueError,'reviewed input drift'):
            policy.activate(self.root)

    def test_changed_raw_audit_is_rejected(self):
        self.audit['full_gate_pass']=True
        self.write('post-review-audit.json',self.audit)
        with self.assertRaisesRegex(ValueError,'raw audit'):
            policy.activate(self.root)

    def test_changed_activation_is_rejected(self):
        policy.activate(self.root)
        path=self.root/policy.ACTIVATION
        value=json.loads(path.read_text());value['actual_scores']['overall']=9
        path.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError,'activation binding changed'):
            policy.verify(self.root)

    def test_first_numeric_verdict_controls_even_if_later_score_is_lower(self):
        later=self.directory.with_name('attempt-08');later.mkdir()
        value=dict(self.verdict,attempt_id='attempt-08',scores={k:1 for k in self.scores})
        (later/'verdict-returned.json').write_text(json.dumps(value))
        value=policy.activate(self.root)
        self.assertEqual(value['scores'],self.scores)


if __name__=='__main__':unittest.main()
