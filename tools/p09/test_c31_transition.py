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
        (self.directory / 'transition.json').write_text(json.dumps(self.seal))
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


if __name__ == '__main__':
    unittest.main()
