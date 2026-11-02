"""Fail-closed transition guard probes; no application/native/live actions."""
from pathlib import Path
import json
import tempfile
import unittest
from unittest.mock import patch
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
import c31_transition_policy as policy
import phase_packet_owner_closure as packet


class TransitionGuards(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.directory = self.root / policy.PREFIX
        self.directory.mkdir(parents=True)
        self.old = {'README.md': {'kind': 'file', 'sha256': 'old', 'mode': 420}}
        self.current = dict(self.old)
        self.commit = 'a' * 40
        self.seal = {'baseline_head': policy.BASE_HEAD, 'baseline_digest': policy.digest(self.old),
                     'source_delta': {}, 'current_source_digest': policy.digest(self.current),
                     'fix_commits': [self.commit, 'b' * 40],
                     'focused_results': [{'exit': 0, 'record': 'preflight.json', 'sha256': 'result'}]}
        (self.root / 'preflight.json').write_text(json.dumps({'command': ['test'], 'exit': 0}))
        self.baseline = {'head': policy.BASE_HEAD, 'files': self.old, 'digest': policy.digest(self.old)}
        (self.directory / 'baseline-source.json').write_text(json.dumps(self.baseline))
        self.addCleanup(patch.stopall)
        patch.object(policy, 'BASE_DIGEST', policy.digest(self.old)).start()
        patch.object(packet, 'source_inventory', return_value=self.current).start()
        patch.object(policy.subprocess, 'run').start().return_value.returncode = 0
        patch.object(policy.subprocess, 'check_output', return_value='').start()
        patch.object(policy.original, 'sha', side_effect=lambda p: policy.OWNER_SHA if str(p).endswith(policy.OWNER_SCORE) else 'result').start()

    def verify(self):
        (self.directory / policy.TRANSITION_RECORD).write_text(json.dumps(self.seal))
        return policy.verify_transition(self.root)

    def test_complete_declared_transition(self):
        self.assertEqual(self.verify()['changed_paths'], [])

    def test_undeclared_current_change_rejected(self):
        self.current['README.md']['sha256'] = 'drift'
        with self.assertRaisesRegex(ValueError, 'sealed C31 delta'):
            self.verify()

    def test_declared_unrelated_production_change_rejected(self):
        self.current['apps/desktop/src/app.tsx'] = {'kind': 'file', 'sha256': 'bad', 'mode': 420}
        self.seal['source_delta'] = {'apps/desktop/src/app.tsx': {'before': None, 'after': self.current['apps/desktop/src/app.tsx']}}
        with self.assertRaisesRegex(ValueError, 'non-C31'):
            self.verify()

    def test_failed_focused_result_rejected(self):
        self.seal['focused_results'][0]['exit'] = 1
        with self.assertRaisesRegex(ValueError, 'preflight'):
            self.verify()

    def test_unintegrated_fix_rejected(self):
        with patch.object(policy.subprocess, 'run') as run:
            run.return_value.returncode = 1
            with self.assertRaisesRegex(ValueError, 'not integrated'):
                self.verify()

    def test_fix_commit_unrelated_path_rejected(self):
        with patch.object(policy.subprocess, 'check_output', return_value='apps/desktop/src/app.tsx\n'):
            with self.assertRaisesRegex(ValueError, 'unrelated source'):
                self.verify()

    def test_late_source_change_rejected(self):
        self.verify()
        self.current['new-source.rs'] = {'kind': 'file', 'sha256': 'new', 'mode': 420}
        with self.assertRaisesRegex(ValueError, 'sealed C31 delta'):
            policy.verify_transition(self.root)



class DriverContinuationGuards(unittest.TestCase):
    root = Path(__file__).resolve().parents[2]

    def current_digest(self):
        return json.loads((self.root / policy.PREFIX / 'driver-amendment-01/equivalence.json').read_text())['after_digest']

    def test_actual_complete_equivalence(self):
        result = policy.verify_driver_amendment(self.root, self.current_digest())
        self.assertEqual(result['retained_native_steps'], 10)

    def test_changed_application_artifact_rejected(self):
        original_sha = policy.original.sha
        prep = json.loads((self.root / 'docs/planning/evidence/P09/native-prepare.json').read_text())
        executable = self.root / prep['bundle'] / 'Contents/MacOS/prisma-airs-desktop-v2'
        with patch.object(policy.original, 'sha', side_effect=lambda p: 'changed' if Path(p) == executable else original_sha(p)):
            with self.assertRaisesRegex(ValueError, 'executable changed'):
                policy.verify_driver_amendment(self.root, self.current_digest())

    def test_changed_runtime_outside_the_one_function_rejected(self):
        original_text = Path.read_text
        driver = self.root / 'tools/native-acceptance/acceptance_c31.py'
        def text(path, *args, **kwargs):
            value = original_text(path, *args, **kwargs)
            return value + '\nUNRELATED_RUNTIME_CHANGE = True\n' if path == driver else value
        with patch.object(Path, 'read_text', text):
            with self.assertRaisesRegex(ValueError, 'exceeds the Gateway'):
                policy.verify_driver_amendment(self.root, self.current_digest())

    def test_changed_retained_evidence_rejected(self):
        read = policy.original.read
        def changed(path):
            value = read(path)
            if str(path).endswith('driver-amendment-01/equivalence.json'):
                value['completed_native_steps'][0]['record_sha256'] = 'wrong'
            return value
        with patch.object(policy.original, 'read', side_effect=changed):
            with self.assertRaisesRegex(ValueError, 'retained G8 evidence changed'):
                policy.verify_driver_amendment(self.root, self.current_digest())

if __name__ == '__main__':
    unittest.main()
