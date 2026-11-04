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

    def c30_accounting_fixture(self):
        # Incomplete unit-only dictionary: no schema, executable, paths or
        # provenance. It cannot qualify as actual performance evidence.
        rows = [{'index': i, 'from': 'Tenant A' if i % 2 == 0 else 'Tenant B',
                 'to': 'Tenant B' if i % 2 == 0 else 'Tenant A', 'warmup': i < 2, 'exit': 0,
                 'probe': {'ms': float(20 + i), 'polls': 2, 'method': 'c30-cached-switch-v2'}} for i in range(12)]
        return {'label': 'acceptance', 'status': 'pass', 'failure': None, 'method': 'c30-cached-switch-v2',
                'metrics': {'tenant-switch-home': {'unit': 'ms', 'samples': [r['probe']['ms'] for r in rows[2:]]}},
                'details': {'switches': 12, 'raw_switches': rows}}

    def test_c30_accounting_keeps_all_trials_and_the_registered_limit(self):
        value = self.c30_accounting_fixture()
        self.assertEqual(phase.c30_samples(value), 31)
        value['details']['raw_switches'][-1]['probe']['ms'] = 101
        value['metrics']['tenant-switch-home']['samples'][-1] = 101
        with self.assertRaisesRegex(ValueError, '100ms'):
            phase.c30_samples(value)

    def test_c30_never_qualifies_calibration_old_or_failed_measurements(self):
        for field, invalid in [('label', 'calibration'), ('status', 'fail'), ('method', 'legacy'), ('failure', 'probe timeout')]:
            value = self.c30_accounting_fixture()
            value[field] = invalid
            with self.subTest(field=field), self.assertRaises(ValueError):
                phase.c30_samples(value)
        with self.assertRaises(ValueError):
            phase.c30_samples(phase.read(ROOT / 'docs/planning/evidence/C30/workload-perf.json'))

    def test_c30_rejects_missing_trials_or_posthoc_sample_selection(self):
        for field in ['missing', 'selected', 'warmup', 'direction', 'exit']:
            value = self.c30_accounting_fixture()
            if field == 'missing': value['details']['raw_switches'].pop()
            elif field == 'selected': value['metrics']['tenant-switch-home']['samples'].reverse()
            elif field == 'warmup': value['details']['raw_switches'][2]['warmup'] = True
            elif field == 'direction': value['details']['raw_switches'][3]['to'] = 'Tenant B'
            else: value['details']['raw_switches'][3]['exit'] = 2
            with self.subTest(field=field), self.assertRaises(ValueError):
                phase.c30_samples(value)

    def test_c30_rejects_nonfinite_boolean_and_nonpositive_timings(self):
        for invalid in [True, float('nan'), float('inf'), 0, -1]:
            value = self.c30_accounting_fixture()
            value['details']['raw_switches'][2]['probe']['ms'] = invalid
            value['metrics']['tenant-switch-home']['samples'][0] = invalid
            with self.subTest(value=invalid), self.assertRaises(ValueError):
                phase.c30_samples(value)

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

    def test_transport_guard_rejects_bypass_connectors_and_changed_final_message(self):
        # Deliberately not a verdict-schema object: these temporary guard
        # fixtures cannot qualify a review and make no model/provider call.
        with tempfile.TemporaryDirectory(prefix='review-transport-guard-') as scratch:
            out = Path(scratch)
            (out / 'request.json').write_text('{}')
            verdict = {'transport_guard_fixture_only': True}
            (out / 'verdict-returned.json').write_text(json.dumps(verdict))
            probe = ''.join(name + ' stable false\n' for name in sorted(phase.REVIEW_DISABLED))
            (out / 'feature-probe.log').write_text(probe)
            argv = ['codex', '--no-daemon', '-a', 'never']
            argv += [arg for name in sorted(phase.REVIEW_DISABLED) for arg in ['--disable', name]]
            argv += ['exec', '-s', 'read-only', '--ephemeral', '--ignore-user-config', '--ignore-rules', '--json',
                     '-m', 'guard-fixture-no-model-call', '--output-schema', 'docs/architecture/review-verdict.schema.json',
                     '-c', 'web_search="disabled"', '-c', 'model_reasoning_effort="xhigh"', '-o', str(out / 'verdict-returned.json')]
            invocation = {'request_sha256': phase.sha(out / 'request.json'),
                          'schema_sha256': phase.sha(ROOT / 'docs/architecture/review-verdict.schema.json'),
                          'command_without_prompt': argv, 'model_requested': 'guard-fixture-no-model-call',
                          'feature_probe_sha256': phase.sha(out / 'feature-probe.log'),
                          'restricted_features_observed': {name: 'false' for name in phase.REVIEW_DISABLED},
                          'harness_version_observed': 'guard fixture only; no actual reviewer'}
            final = {'type': 'item.completed', 'item': {'type': 'agent_message', 'text': json.dumps(verdict)}}
            (out / 'invocation.json').write_text(json.dumps(invocation))
            (out / 'thread-reply.log').write_text(json.dumps(final) + '\n')
            phase.verify_launch(ROOT, out)
            for extra in [['--dangerously-bypass-approvals-and-sandbox'], ['-s', 'workspace-write']]:
                altered = copy.deepcopy(invocation)
                altered['command_without_prompt'] += extra
                (out / 'invocation.json').write_text(json.dumps(altered))
                with self.subTest(extra=extra), self.assertRaises(ValueError):
                    phase.verify_launch(ROOT, out)
            (out / 'invocation.json').write_text(json.dumps(invocation))
            connector = {'type': 'item.completed', 'item': {'type': 'mcp_tool_call'}}
            (out / 'thread-reply.log').write_text(json.dumps(connector) + '\n' + json.dumps(final) + '\n')
            with self.assertRaises(ValueError):
                phase.verify_launch(ROOT, out)
            final['item']['text'] = '{}'
            (out / 'thread-reply.log').write_text(json.dumps(final) + '\n')
            with self.assertRaises(ValueError):
                phase.verify_launch(ROOT, out)


if __name__ == '__main__':
    unittest.main()
