"""Guard regressions using retained verdicts; never native acceptance evidence."""
from pathlib import Path
import copy
import json
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / 'tools/p09'))
sys.path.insert(0, str(ROOT / 'tools/native-acceptance'))
sys.path.insert(0, str(ROOT / 'docs/planning/evidence/P09/native-controller'))
import phase_inputs as phase
import phase_packet as packet


class PhaseInputGuards(unittest.TestCase):
    def setUp(self):
        self.verdict = phase.read(ROOT / 'docs/planning/reviews/C44/attempt-05/verdict-returned.json')

    def test_actual_full_gate_and_retained_accepted_eight(self):
        phase.full_gate(self.verdict)
        with self.assertRaises(ValueError):
            phase.full_gate(phase.read(ROOT / 'docs/planning/reviews/C29/attempt-07/verdict-returned.json'))

    def test_each_dimension_and_findings_are_mandatory(self):
        for dimension in phase.DIMENSIONS:
            for invalid in [8, True, '9', 11]:
                value = copy.deepcopy(self.verdict)
                value['scores'][dimension] = invalid
                with self.subTest(dimension=dimension, invalid=invalid), self.assertRaises(ValueError):
                    phase.full_gate(value)
        for field in ['required_fixes', 'blockers', 'evidence_gaps']:
            for invalid in [['unresolved'], None]:
                value = copy.deepcopy(self.verdict)
                value[field] = invalid
                with self.subTest(field=field), self.assertRaises(ValueError):
                    phase.full_gate(value)

    def test_missing_dimension_or_nonpassing_recommendation(self):
        value = copy.deepcopy(self.verdict)
        del value['scores']['validation']
        with self.assertRaises(ValueError):
            phase.full_gate(value)
        value = copy.deepcopy(self.verdict)
        value['recommendation'] = 'fail'
        with self.assertRaises(ValueError):
            phase.full_gate(value)

    def test_historical_kirk_check_run_cannot_qualify_current_clone(self):
        with self.assertRaisesRegex(ValueError, 'active clone'):
            phase.verify_checks(ROOT, ROOT / 'docs/planning/evidence/P09/runs/run-attempt-02.json')

    def test_native_executable_mismatch_fails_before_records(self):
        prepared = phase.read(ROOT / 'docs/planning/evidence/P09/native-prepare.json')
        prepared['bundle'] = str(ROOT / prepared['bundle'])
        prepared['executable_sha256'] = '0' * 64
        with tempfile.TemporaryDirectory(prefix='p09-phase-guard-') as scratch:
            root = Path(scratch)
            directory = root / 'docs/planning/evidence/P09'
            directory.mkdir(parents=True)
            (directory / 'native-prepare.json').write_text(json.dumps(prepared))
            with self.assertRaisesRegex(ValueError, 'executable drift'):
                phase.verify_native(root)

    def test_packet_refuses_historical_checks_without_creating_attempt(self):
        attempt = 'attempt-999999'
        out = ROOT / 'docs/planning/reviews/P09' / attempt
        self.assertFalse(out.exists())
        with self.assertRaisesRegex(ValueError, 'active clone'):
            packet.make(ROOT, attempt, ROOT / 'docs/planning/evidence/P09/runs/run-attempt-02.json', [])
        self.assertFalse(out.exists())

    def test_phase_schema_rejects_extra_fields_and_boolean_scores(self):
        schema = phase.read(ROOT / 'docs/architecture/review-verdict.schema.json')
        packet.validate(self.verdict, schema)
        value = copy.deepcopy(self.verdict)
        value['approved'] = True
        with self.assertRaises(ValueError):
            packet.validate(value, schema)
        value = copy.deepcopy(self.verdict)
        value['scores']['validation'] = True
        with self.assertRaises(ValueError):
            packet.validate(value, schema)

    def test_source_identity_binds_modes_and_symlink_targets(self):
        with tempfile.TemporaryDirectory(prefix='p09-source-guard-') as scratch:
            path = Path(scratch) / 'source'
            path.write_text('same bytes')
            before = packet.file_identity(path)
            path.chmod(0o700)
            self.assertNotEqual(before, packet.file_identity(path))
            link = Path(scratch) / 'link'
            link.symlink_to('source')
            before = packet.file_identity(link)
            link.unlink()
            link.symlink_to('different-source')
            self.assertNotEqual(before, packet.file_identity(link))

    def test_active_controller_and_check_sources_are_not_hidden_as_evidence(self):
        inventory = packet.source_inventory(ROOT)
        self.assertIn('docs/planning/evidence/P09/native-controller/run-p09-native.py', inventory)
        self.assertIn('docs/planning/evidence/P09/runs/hash-inputs.sh', inventory)
        self.assertNotIn('docs/planning/evidence/P09/native-prepare.json', inventory)


if __name__ == '__main__':
    unittest.main()
