"""Synthetic validator fixtures only; no app, review session, or UI is run.

The existing audited-review transport and legacy phase consumer are substituted
at their boundary. Source/application/coverage/raw-evidence checks run for real.
"""
from pathlib import Path
import copy
import json
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import standing as s
import proof


class StandingTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        self.source = self.root / s.ACCEPTED / 'source'
        self.source.mkdir(parents=True)
        self.attempts = {name: 'attempt-01' for name in s.SUBJECTS}
        self.baseline = {'fixture': 'previous independently audited standing'}

        def write(path, value):
            path = self.root / path
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(value) + '\n')
            return s.reference(self.root, path)
        self.write = write
        self.helper = 'tools/native-acceptance/acceptance_p11.py'
        helper = self.source / self.helper
        helper.parent.mkdir(parents=True)
        helper.write_text('# synthetic archival fixture; never executed\n')
        manifests = self.source / 'docs/architecture'
        manifests.mkdir(parents=True)
        manifest = {'setup_commands': [], 'phases': {'P11': list(s.CHUNKS)}, 'chunks': {}}
        natives = {'chunks': {}}
        for name in s.CHUNKS:
            manifest['chunks'][name] = {'check_commands': [['fixture-check', name]],
                'fixture_command': ['cargo', 'test', '--test', name]}
            natives['chunks'][name] = [{'id': f'{name}-N{i:02}', 'check': 'validator fixture'} for i in range(1, 4)]
        (manifests / 'checks.json').write_text(json.dumps(manifest))
        (manifests / 'native-checks.json').write_text(json.dumps(natives))
        files = {str(p.relative_to(self.source)): s.file_identity(p)
                 for p in self.source.rglob('*') if p.is_file()}
        self.source_hash = s.digest(files)
        seal = write(s.ACCEPTED / 'source-seal.json', {'files': files})
        app = self.root / s.ACCEPTED / 'application'
        exe = app / 'Contents/MacOS/prisma-airs-desktop'
        exe.parent.mkdir(parents=True)
        exe.write_text('synthetic candidate bytes; never executed')
        self.bundle = app
        bundle_hash = s.bundle_hash(app)
        self.context = write(s.ACCEPTED / 'historical-context.json', {
            'schema': 'airs-desktop/historical-qualification-context/1',
            'sources': {self.source_hash: {'path': str(self.source.relative_to(self.root)), 'seal': seal}},
            'applications': {bundle_hash: {'path': str(app.relative_to(self.root)), 'executable_sha256': s.sha(exe)}}})
        prepared = write(s.BASE / 'fixture/prepare.json', {'bundle': 'target/never-present.app',
            'executable_sha256': s.sha(exe), 'bundle_sha256': bundle_hash})
        inputs = {'source': 'synthetic-original-runner-source', 'dependencies': 'synthetic-deps', 'build_flags': 'synthetic-flags'}
        candidate = {'source_inventory_sha256': self.source_hash, 'execution_root': '/original/qualification-host',
            'runner_inputs': inputs, 'prepare': prepared, 'executable_sha256': s.sha(exe),
            'bundle_sha256': bundle_hash, 'build_flags': {'target': 'synthetic-arm64'},
            'conditions': {'platform': 'original-fixture-platform'}, 'helper_closure': {self.helper: s.sha(helper)}}
        candidate_ref = write(s.BASE / 'fixture/candidate.json', candidate)
        execution = {'status': 'observed', 'exit_code': 0,
            'argv': ['python3', 'tools/native-acceptance/acceptance_p11.py'],
            'executable_sha256': candidate['executable_sha256'], 'bundle_sha256': bundle_hash,
            'helper_closure': candidate['helper_closure'], 'execution_id': 'fixture-native',
            'started_utc': 'fixture-start', 'finished_utc': 'fixture-finish'}
        execution_ref = write(s.BASE / 'fixture/native-execution.json', execution)
        self.requirements, rows = [], []
        for kind in ('check', 'test'):
            commands = []
            needed_commands = proof.command_plan(self.source, kind)
            raw_path = s.BASE / f'fixture/{kind}/run.json'
            for needed in needed_commands:
                label = needed['label']
                relative_log = 'logs/' + label.replace('/', '-') + '.log'
                log = self.root / raw_path.parent / relative_log
                log.parent.mkdir(parents=True, exist_ok=True)
                log.write_text('Synthetic guard fixture only, never an application test pass.\n')
                commands.append({'label': label, 'argv': needed['argv'], 'cwd': '.', 'status': 'passed',
                    'exit_code': 0, 'note': None, 'log': relative_log, 'output_sha256': s.sha(log),
                    'tests': {'passed': 1, 'failed': 0} if needed['expects_tests'] else None})
            raw_ref = write(raw_path, {'schema': 'airs-v2/run/1', 'status': 'passed', 'failure': None,
                'inputs_before': inputs, 'inputs_after': inputs, 'commands': commands})
            for needed, command in zip(needed_commands, commands):
                cid = kind + ':' + needed['label']
                self.requirements.append(dict(id=cid, kind='command', affected=True, must_execute=True, **needed))
                result = {'status': 'pass', 'exit_code': 0, 'execution_id': 'fixture-' + cid,
                    'argv': needed['argv'], 'cwd': candidate['execution_root'], 'started_utc': 'fixture-start',
                    'finished_utc': 'fixture-finish', 'required_ids': [cid], 'raw': raw_ref,
                    'log': s.reference(self.root, self.root / raw_path.parent / command['log']),
                    'candidate': candidate_ref, 'evidence': {}}
                result_ref = write(s.BASE / 'fixture/results' / (cid.replace('/', '-') + '.json'), result)
                rows.append({'id': cid, 'disposition': 'executed', 'result': result_ref})
        native_ids = [row['id'] for name in s.CHUNKS for row in natives['chunks'][name]]
        native_ref = write(s.BASE / 'fixture/native.json', {'schema': 'airs-v2/native-acceptance/1',
            'app': {'executable_sha256': candidate['executable_sha256']},
            'checks': [{'id': cid, 'result': 'pass', 'observation': 'synthetic validator fixture',
                        'evidence': [execution_ref['path']]} for cid in native_ids]})
        for cid in native_ids:
            self.requirements.append({'id': cid, 'kind': 'native', 'argv': execution['argv'],
                                      'affected': True, 'must_execute': True})
            result_ref = write(s.BASE / 'fixture/results' / (cid + '.json'), {
                'status': 'pass', 'exit_code': 0, 'execution_id': 'fixture-native', 'argv': execution['argv'],
                'cwd': candidate['execution_root'], 'started_utc': 'fixture-start', 'finished_utc': 'fixture-finish',
                'required_ids': [cid], 'raw': native_ref, 'log': execution_ref, 'candidate': candidate_ref,
                'evidence': {execution_ref['path']: execution_ref['sha256']}})
            rows.append({'id': cid, 'disposition': 'executed', 'result': result_ref})
        plan = write(s.BASE / 'fixture/plan.json', {'schema': 'airs-desktop/required-coverage/1',
                                                   'requirements': self.requirements})
        coverage = {'schema': 'airs-desktop/qualified-coverage/1', 'plan': plan, 'candidate': candidate_ref,
                    'source_inventory_sha256': self.source_hash, 'coverage': rows}
        coverage_ref = write(s.BASE / 'fixture/coverage.json', coverage)
        self.qualified = {'record': str(s.BASE / 'fixture/qualification.json'), 'sha256': None,
            'verified': s.verify_coverage(self.root, coverage), 'candidate_sha256': candidate_ref['sha256'],
            'controller_complete': False}
        original = write(self.qualified['record'], {'schema': 'airs-desktop/p11-shared-qualification/1',
            'coverage': coverage_ref, 'source': seal, 'native_execution': execution_ref, 'historical_standing': self.baseline})
        self.qualified['sha256'] = original['sha256']
        self.reviews = {name: {'subject': name, 'attempt': 'attempt-01',
            'fingerprints': {'source': self.source_hash},
            'scores': {k: 9 for k in ('architecture', 'behavior', 'authority_security', 'validation', 'overall')}}
            for name in s.SUBJECTS}
        for name in s.SUBJECTS:
            write(Path('docs/planning/reviews') / name / 'attempt-01/request.json', {
                'qualification': self.qualified, 'predecessors': {'members': {c: self.reviews[c] for c in s.CHUNKS}}})
        evidence = write(s.ACCEPTED / 'evidence-inventory.json', {
            'files': s.identities(self.root, s.evidence_files(self.root, self.attempts))})
        self.record = {'schema': 'airs-desktop/p11-completion-proof/1', 'controller_complete': False,
            'context': self.context, 'qualification': self.qualified, 'attempts': self.attempts,
            'reviews': self.reviews, 'historical_standing': self.baseline, 'evidence': evidence}
        write(s.COMPLETION, self.record)
        self.review_mock = patch.object(s, 'actual_review', side_effect=lambda root, subject, attempt, **kw: copy.deepcopy(self.reviews[subject])).start()
        self.addCleanup(patch.stopall)
        patch.object(s, 'release_prerequisites', return_value=self.baseline).start()

    def test_archived_standing_needs_neither_target_nor_current_source(self):
        self.assertFalse((self.root / 'target').exists())
        (self.root / 'future-source.rs').write_text('unrelated subsequent release work')
        result = s.verify(self.root)
        self.assertFalse(result['controller_complete'])
        self.assertEqual(self.review_mock.call_count, 5)

    def test_missing_standing_and_foreign_context_rejected(self):
        completion = self.root / s.COMPLETION
        completion.unlink()
        with self.assertRaisesRegex(ValueError, 'actual accepted standing'):
            s.verify(self.root)
        self.write(s.COMPLETION, dict(self.record, context=dict(self.context, path='foreign/context.json')))
        with self.assertRaisesRegex(ValueError, 'context is foreign'):
            s.verify(self.root)

    def test_source_and_bundle_drift_rejected(self):
        helper = self.source / self.helper
        helper.write_text('drift')
        with self.assertRaisesRegex(ValueError, 'archive membership/content drift'):
            s.verify(self.root)
        helper.write_text('# synthetic archival fixture; never executed\n')
        (self.bundle / 'Contents/extra-resource').write_text('drift')
        with self.assertRaisesRegex(ValueError, 'complete bundle drift'):
            s.verify(self.root)

    def test_raw_log_and_unbound_evidence_rejected(self):
        log = next((self.root / s.BASE / 'fixture/check/logs').glob('*.log'))
        original = log.read_text()
        log.write_text('altered')
        with self.assertRaises(ValueError):
            s.verify(self.root)
        log.write_text(original)
        evidence = s.read(self.root / self.record['evidence']['path'])
        evidence['files'].pop(next(iter(evidence['files'])))
        self.record['evidence'] = self.write(self.record['evidence']['path'], evidence)
        self.write(s.COMPLETION, self.record)
        with self.assertRaisesRegex(ValueError, 'evidence membership differs'):
            s.verify(self.root)

    def test_member_foreign_qualification_latest_and_full_nine_required(self):
        request_path = Path('docs/planning/reviews/C46/attempt-01/request.json')
        request = s.read(self.root / request_path)
        self.write(request_path, dict(request, qualification=dict(self.qualified, sha256='foreign')))
        with self.assertRaisesRegex(ValueError, 'one shared qualification'):
            s.audited_reviews(self.root, self.attempts, self.qualified, self.source)
        self.write(request_path, request)
        self.reviews['C46']['scores']['overall'] = 8
        with self.assertRaisesRegex(ValueError, 'full-9'):
            s.audited_reviews(self.root, self.attempts, self.qualified, self.source)
        self.reviews['C46']['scores']['overall'] = 9
        (self.root / 'docs/planning/reviews/C46/attempt-02').mkdir()
        with self.assertRaisesRegex(ValueError, 'newer review attempt'):
            s.audited_reviews(self.root, self.attempts, self.qualified, self.source)

    def test_phase_must_bind_exact_audited_members(self):
        path = Path('docs/planning/reviews/P11/attempt-01/request.json')
        request = s.read(self.root / path)
        request['predecessors']['members']['C46']['attempt'] = 'foreign'
        self.write(path, request)
        with self.assertRaisesRegex(ValueError, 'exact four audited member'):
            s.audited_reviews(self.root, self.attempts, self.qualified, self.source)

    def test_legacy_context_default_stays_constrained_to_c45(self):
        legacy = s.Historical(self.root, self.context)
        with self.assertRaisesRegex(ValueError, 'escapes C45 evidence'):
            legacy.source(self.source_hash)
        self.assertEqual(s.ArchivedP11(self.root, self.context).source(self.source_hash), self.source)

    def test_finalize_refuses_to_overwrite_any_accepted_output(self):
        before = (self.root / s.COMPLETION).read_bytes()
        with self.assertRaisesRegex(ValueError, 'preserve existing'):
            s.finalize(self.root, self.root / self.qualified['record'], self.attempts)
        self.assertEqual((self.root / s.COMPLETION).read_bytes(), before)

    def test_large_archive_paths_require_explicit_lfs_rules(self):
        subprocess.run(['/usr/bin/git', 'init', '-q'], cwd=self.root, check=True)
        with self.assertRaisesRegex(ValueError, 'archive Git LFS paths'):
            s.require_archive_lfs(self.root)
        (self.root / '.gitattributes').write_text(''.join(name + ' filter=lfs -text\n' for name in s.LFS_PATHS))
        s.require_archive_lfs(self.root)


if __name__ == '__main__':
    unittest.main()
