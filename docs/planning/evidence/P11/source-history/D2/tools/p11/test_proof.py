"""Adversarial proof-consumer fixtures; these are not application test results."""
from pathlib import Path
import copy
import json
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

import proof


class ProofTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        source = Path(__file__).resolve().parents[2]
        folder = self.root / 'docs/architecture'
        folder.mkdir(parents=True)
        for name in ('checks.json', 'native-checks.json'):
            shutil.copyfile(source / 'docs/architecture' / name, folder / name)
        self.inputs = {'source': 'fixture-source', 'dependencies': 'fixture-dependencies',
                       'build_flags': 'fixture-flags'}

    def run_fixture(self, kind):
        folder = self.root / 'fixture-run'
        folder.mkdir()
        commands = []
        for i, needed in enumerate(proof.command_plan(self.root, kind)):
            log = folder / f'{i}.log'
            log.write_text('validator fixture only\n')
            commands.append(dict(label=needed['label'], argv=needed['argv'], cwd='.',
                                 status='passed', exit_code=0, note=None, log=log.name,
                                 output_sha256=proof.sha(log),
                                 tests={'passed': 1, 'failed': 0} if needed['expects_tests'] else None))
        raw = dict(schema='airs-v2/run/1', subject='P11', kind=kind, status='passed', failure=None,
                   inputs_before=self.inputs, inputs_after=self.inputs, commands=commands)
        path = folder / 'run.json'
        path.write_text(json.dumps(raw))
        return path, raw

    def test_exact_plan_and_logs_are_required(self):
        path, raw = self.run_fixture('test')
        proof.validate_run(self.root, path, 'test', self.inputs)
        for change in ('omit', 'duplicate', 'zero', 'input', 'subject'):
            altered = copy.deepcopy(raw)
            if change == 'omit':
                altered['commands'].pop()
            elif change == 'duplicate':
                altered['commands'].append(altered['commands'][-1])
            elif change == 'zero':
                next(row for row in altered['commands'] if row['tests'])['tests']['passed'] = 0
            elif change == 'input':
                altered['inputs_after'] = dict(self.inputs, source='drift')
            else:
                altered['subject'] = 'C46'
            path.write_text(json.dumps(altered))
            with self.subTest(change=change), self.assertRaises(ValueError):
                proof.validate_run(self.root, path, 'test', self.inputs)
        path.write_text(json.dumps(raw))
        (path.parent / raw['commands'][0]['log']).write_text('changed log')
        with self.assertRaisesRegex(ValueError, 'log drift'):
            proof.validate_run(self.root, path, 'test', self.inputs)

    def test_no_legacy_common_replay_or_unselected_rust_suite(self):
        checks = proof.command_plan(self.root, 'check')
        tests = proof.command_plan(self.root, 'test')
        self.assertTrue(all(row['label'].startswith(('setup/',) + proof.CHUNKS) for row in checks + tests))
        self.assertEqual(len({tuple(row['argv']) for row in tests}), len(tests))
        for row in checks + tests:
            argv = row['argv']
            self.assertNotIn('--all-targets', argv)
            self.assertNotIn('--workspace', argv)
            if argv[:2] == ['cargo', 'test']:
                self.assertTrue('--test' in argv or ('--lib' in argv and argv[-1] != '--lib'))

    def test_every_new_native_assertion_must_be_present_and_affected(self):
        rows = []
        for kind in ('check', 'test'):
            rows += [dict(id=kind + ':' + row['label'], kind='command', affected=True,
                          must_execute=True, **row) for row in proof.command_plan(self.root, kind)
                     if not row['label'].startswith('setup/')]
        native = proof.read(self.root / 'docs/architecture/native-checks.json')['chunks']
        rows += [dict(id=row['id'], kind='native', affected=True, must_execute=True)
                 for chunk in proof.CHUNKS for row in native[chunk]]
        plan = {'requirements': rows}
        proof.verify_plan(self.root, plan)
        self.assertEqual(sum(row['kind'] == 'native' for row in rows), 12)
        for altered in [rows[:-1], rows + [rows[-1]], [dict(row, affected=False) for row in rows]]:
            with self.assertRaises(ValueError):
                proof.verify_plan(self.root, {'requirements': altered})

    def test_current_runner_source_hash_changes_on_untracked_edit_and_deletion(self):
        subprocess.run(['/usr/bin/git', 'init', '-q'], cwd=self.root, check=True)
        file = self.root / 'actual-source.rs'
        file.write_text('first')
        before = proof.xtask_source(self.root)
        file.write_text('second')
        changed = proof.xtask_source(self.root)
        self.assertNotEqual(before, changed)
        subprocess.run(['/usr/bin/git', 'add', 'actual-source.rs'], cwd=self.root, check=True)
        self.assertEqual(changed, proof.xtask_source(self.root))
        file.unlink()
        self.assertNotEqual(changed, proof.xtask_source(self.root))

    def test_failed_native_process_cannot_be_wrapped_as_a_pass(self):
        candidate = dict(executable_sha256='fixture-executable', bundle_sha256='fixture-bundle',
                         helper_closure={'fixture-helper': 'fixture-hash'})
        execution = dict(status='observed', exit_code=0,
                         argv=['python3', 'tools/native-acceptance/acceptance_p11.py'], **candidate)
        proof.validate_execution(self.root, execution, candidate)
        for altered in [dict(execution, status='failed'), dict(execution, exit_code=1),
                        dict(execution, bundle_sha256='different'),
                        dict(execution, helper_closure={})]:
            with self.assertRaises(ValueError):
                proof.validate_execution(self.root, altered, candidate)

    def frontend_carry_fixture(self):
        """Small byte-bound fixture, without executing a frontend/native build."""
        subprocess.run(['/usr/bin/git', 'init', '-q'], cwd=self.root, check=True)
        (self.root / '.gitignore').write_text('apps/desktop/dist/\n.evidence-private/\n')
        name = 'apps/desktop/src/example.test.tsx'
        test = self.root / name
        test.parent.mkdir(parents=True)
        test.write_text('original test\n')
        before = proof.sha(test)
        test.write_text('await the real publication\n')
        dist = self.root / 'apps/desktop/dist/index.html'
        dist.parent.mkdir()
        dist.write_text('unchanged application frontend\n')
        folder = self.root / 'docs/fixture-carry'
        folder.mkdir()
        build_path = folder / 'build.json'
        build = {'application_inputs_before': {name: before}}
        build_path.write_text(json.dumps(build))
        prepare = folder / 'prepare.json'
        prepare.write_text('{}\n')
        candidate = {'prepare': proof.reference(self.root, prepare)}
        log = folder / 'build.log'
        log.write_text('frontend build fixture only\n')
        inputs = {item: proof.sha(self.root / item)
                  for item in proof.application_files(self.root)}
        equivalence = {
            'schema': 'airs-desktop/frontend-build-equivalence/1',
            'cwd': str(self.root),
            'argv': ['pnpm', '--filter', '@prisma-airs/desktop', 'exec', 'vite',
                     'build', '--outDir', str(self.root / '.evidence-private/rebuilt')],
            'application_inputs_before': inputs,
            'application_inputs_after': inputs,
            'exit_code': 0,
            'frontend_byte_identical': True,
            'original_frontend_unchanged': True,
            'original_prepare_sha256': candidate['prepare']['sha256'],
            'original_frontend_files': {'index.html': proof.sha(dist)},
            'rebuilt_frontend_files': {'index.html': proof.sha(dist)},
            'log_sha256': proof.sha(log),
        }
        equivalence_path = folder / 'equivalence.json'
        carry_path = folder / 'carry.json'
        carry = {
            'schema': 'airs-desktop/frontend-test-build-carry/1',
            'build': proof.reference(self.root, build_path),
            'prepare': candidate['prepare'],
            'native_rebuilt': False,
            'changed_inputs': {name: {'before': before, 'after': proof.sha(test)}},
            'reason': 'Test-only readiness correction; actual frontend bytes retained.',
        }
        record = {}

        def bind():
            equivalence_path.write_text(json.dumps(equivalence))
            carry['frontend_equivalence'] = proof.reference(self.root, equivalence_path)
            carry_path.write_text(json.dumps(carry))
            record['build_input_carry'] = proof.reference(self.root, carry_path)

        def verify(changed=None):
            proof.frontend_test_carry(self.root, record, build_path, build, candidate,
                                      changed if changed is not None else [name])

        bind()
        return bind, verify, equivalence, carry, name, dist, log

    def test_frontend_test_carry_requires_exact_delta_and_candidate(self):
        bind, verify, equivalence, carry, name, _, _ = self.frontend_carry_fixture()
        verify()
        for changed in ([], ['apps/desktop/src/application.tsx']):
            with self.subTest(changed=changed), self.assertRaises(ValueError):
                verify(changed)
        original = copy.deepcopy(carry)
        for change in ('delta', 'candidate', 'build', 'rebuilt'):
            carry.clear()
            carry.update(copy.deepcopy(original))
            if change == 'delta':
                carry['changed_inputs'][name]['after'] = 'another test edit'
            elif change == 'candidate':
                carry['prepare']['sha256'] = 'another candidate'
            elif change == 'build':
                carry['build']['sha256'] = 'another build'
            else:
                carry['native_rebuilt'] = True
            bind()
            with self.subTest(change=change), self.assertRaises(ValueError):
                verify()

    def test_frontend_equivalence_requires_real_build_and_current_inputs(self):
        bind, verify, equivalence, _, name, _, _ = self.frontend_carry_fixture()
        original = copy.deepcopy(equivalence)
        for change in ('schema', 'cwd', 'command', 'extra-argument', 'outside-output',
                       'old-input', 'input-drift', 'input-omission'):
            equivalence.clear()
            equivalence.update(copy.deepcopy(original))
            if change == 'schema':
                equivalence['schema'] = 'unrelated/1'
            elif change == 'cwd':
                equivalence['cwd'] = str(self.root.parent)
            elif change == 'command':
                equivalence['argv'][4:6] = ['echo', 'success']
            elif change == 'extra-argument':
                equivalence['argv'].append('--mode=test')
            elif change == 'outside-output':
                equivalence['argv'][-1] = str(self.root.parent / 'different-output')
            elif change == 'old-input':
                equivalence['application_inputs_before'][name] = 'old test hash'
                equivalence['application_inputs_after'][name] = 'old test hash'
            elif change == 'input-drift':
                equivalence['application_inputs_after'] = dict(equivalence['application_inputs_after'],
                                                      **{name: 'changed during build'})
            else:
                equivalence['application_inputs_before'] = {}
                equivalence['application_inputs_after'] = {}
            bind()
            with self.subTest(change=change), self.assertRaises(ValueError):
                verify()

    def test_frontend_equivalence_requires_success_exact_bytes_and_original_prepare(self):
        bind, verify, equivalence, _, _, dist, log = self.frontend_carry_fixture()
        original = copy.deepcopy(equivalence)
        for change in ('failure', 'byte-difference', 'missing-output', 'candidate'):
            equivalence.clear()
            equivalence.update(copy.deepcopy(original))
            if change == 'failure':
                equivalence['exit_code'] = 1
            elif change == 'byte-difference':
                equivalence['rebuilt_frontend_files']['index.html'] = 'different bytes'
            elif change == 'missing-output':
                equivalence['rebuilt_frontend_files'] = {}
            else:
                equivalence['original_prepare_sha256'] = 'another candidate'
            bind()
            with self.subTest(change=change), self.assertRaises(ValueError):
                verify()
        equivalence.clear()
        equivalence.update(copy.deepcopy(original))
        bind()
        log.write_text('replaced log\n')
        with self.assertRaisesRegex(ValueError, 'log drift'):
            verify()
        log.write_text('frontend build fixture only\n')
        dist.write_text('changed retained frontend\n')
        with self.assertRaisesRegex(ValueError, 'retained frontend bytes changed'):
            verify()

    def check_selection_fixture(self):
        """Synthetic partial-run carry; none of these fixture commands execute."""
        subprocess.run(['/usr/bin/git', 'init', '-q'], cwd=self.root, check=True)
        module = self.root / 'crates/desktop-core/src/authority/setup_warning_domain.rs'
        module.parent.mkdir(parents=True)
        module.write_text('before conditional style fixture\n')
        before_module = proof.sha(module)
        original_path, original = self.run_fixture('check')
        folder = self.root / 'docs/planning/evidence/P11/check-fixture'
        folder.mkdir(parents=True)
        original_path.parent.rename(folder / 'original')
        original_path = folder / 'original/run.json'
        before = {'files': proof.source_inventory(self.root),
                  'git_config': proof.file_identity(self.root / '.git/config')}
        original.update(run_id='fixture-original', started_utc='fixture-start',
                        finished_utc='fixture-end', status='failed', failure='C46/check/6')
        original['inputs_before'] = original['inputs_after'] = dict(self.inputs, source=proof.inventory_source(before))
        failed = False
        for row in original['commands']:
            if row['label'] == 'C46/check/6':
                failed = True
                row.update(status='failed', exit_code=101)
            elif failed:
                row.update(status='blocked', exit_code=None)
        module.write_text('after conditional style fixture\n')
        self.addCleanup(patch.stopall)
        patch.object(proof, 'CHECK_LINT_SHA256', (before_module, proof.sha(module))).start()
        after = {'files': proof.source_inventory(self.root), 'git_config': before['git_config']}
        inputs = dict(self.inputs, source=proof.inventory_source(after))
        supplement_path = folder / 'supplement/run.json'
        supplement_path.parent.mkdir()
        supplement = dict(schema='airs-desktop/p11-check-supplement/1', subject='P11', kind='check',
                          run_id='fixture-supplement', started_utc='fixture-new-start',
                          finished_utc='fixture-new-end', status='passed', failure=None,
                          inputs_before=inputs, inputs_after=inputs, commands=[])
        rows = []
        for i, needed in enumerate(proof.command_plan(self.root, 'check')):
            mode = 'carried_unaffected' if needed['label'] in proof.CHECK_CARRY_LABELS else 'executed'
            rows.append(dict(label=needed['label'], disposition=mode,
                             reason='Synthetic unchanged-impact fixture.' if mode == 'carried_unaffected' else None))
            if mode == 'executed':
                log = supplement_path.parent / f'{i}.log'
                log.write_text('Synthetic current check fixture, never executed.\n')
                supplement['commands'].append(dict(label=needed['label'], argv=needed['argv'], cwd='.',
                    status='passed', exit_code=0, note=None, log=log.name,
                    output_sha256=proof.sha(log), tests=None))
        selection_path = folder / 'selection.json'
        selection = dict(schema='airs-desktop/p11-check-selection/1', execution_root=str(self.root),
                         runner_inputs=inputs, changed_paths=[str(module.relative_to(self.root))],
                         reason='Synthetic evidence-carry fixture.', rows=rows)

        def bind():
            for name, value in [('before', before), ('after', after)]:
                p = folder / (name + '.json')
                p.write_text(json.dumps(value))
                selection['original_source' if name == 'before' else 'current_source'] = proof.reference(self.root, p)
            original_path.write_text(json.dumps(original))
            supplement_path.write_text(json.dumps(supplement))
            selection['original_run'] = proof.reference(self.root, original_path)
            selection['supplement'] = proof.reference(self.root, supplement_path)
            selection_path.write_text(json.dumps(selection))

        def verify():
            return proof.validate_run(self.root, selection_path, 'check', inputs)
        bind()
        return bind, verify, selection, original, supplement, after, module

    def test_check_selection_preserves_failed_run_and_rejects_invalid_carry(self):
        bind, verify, selection, original, supplement, after, module = self.check_selection_fixture()
        result = verify()
        self.assertEqual(original['status'], 'failed')
        self.assertEqual(sum(r['disposition'] == 'carried_unaffected' for r in result['selected']), 6)
        baseline = copy.deepcopy(selection)
        for change in ('omit', 'duplicate', 'failed-carry', 'missing-reason', 'undeclared', 'wrong-root'):
            selection.clear(); selection.update(copy.deepcopy(baseline))
            if change == 'omit': selection['rows'].pop()
            elif change == 'duplicate': selection['rows'].append(selection['rows'][-1])
            elif change == 'failed-carry':
                next(r for r in selection['rows'] if r['label'] == 'C46/check/6')['disposition'] = 'carried_unaffected'
            elif change == 'missing-reason': selection['rows'][0]['reason'] = None
            elif change == 'undeclared': selection['changed_paths'] = []
            else: selection['execution_root'] = str(self.root.parent)
            bind()
            with self.subTest(change=change), self.assertRaises(ValueError): verify()
        selection.clear(); selection.update(baseline); bind()
        original['commands'][0]['status'] = 'failed'; bind()
        with self.assertRaisesRegex(ValueError, 'selected raw check command failed'): verify()

    def test_check_selection_rejects_source_dependency_supplement_and_log_drift(self):
        bind, verify, selection, original, supplement, after, module = self.check_selection_fixture()
        old = copy.deepcopy(supplement)
        for change in ('inputs', 'missing-command', 'failed-command', 'wrong-argv'):
            supplement.clear(); supplement.update(copy.deepcopy(old))
            if change == 'inputs': supplement['inputs_after'] = dict(supplement['inputs_after'], source='old')
            elif change == 'missing-command': supplement['commands'].pop()
            elif change == 'failed-command': supplement['commands'][0]['exit_code'] = 1
            else: supplement['commands'][0]['argv'] = ['echo', 'success']
            bind()
            with self.subTest(change=change), self.assertRaises(ValueError): verify()
        supplement.clear(); supplement.update(old); bind()
        original['inputs_before']['dependencies'] = 'different dependencies'; bind()
        with self.assertRaisesRegex(ValueError, 'dependencies or flags'): verify()
        original['inputs_before']['dependencies'] = self.inputs['dependencies']; bind()
        log = self.root / selection['supplement']['path']
        log = log.parent / supplement['commands'][0]['log']
        log.write_text('replaced raw output\n')
        with self.assertRaisesRegex(ValueError, 'log drift'): verify()
        module.write_text('a subsequent behavior change\n')
        with self.assertRaisesRegex(ValueError, 'current source differs'): verify()

    def test_rust_test_build_carry_requires_exact_delta_and_current_checks(self):
        """Synthetic bindings only; no fixture result is an application pass."""
        test = self.root / proof.CHECK_TEST_PATH
        test.parent.mkdir(parents=True)
        test.write_text('before test-only fixture\n')
        before = proof.sha(test)
        test.write_text('after test-only fixture\n')
        after = proof.sha(test)
        with patch.object(proof, 'CHECK_TEST_LINT_SHA256', (before, after)):
            folder = self.root / 'fixture-rust-carry'
            folder.mkdir()
            build = {'application_inputs_before': {proof.CHECK_TEST_PATH: before}}
            build_path = folder / 'build.json'
            build_path.write_text(json.dumps(build))
            prepare = folder / 'prepare.json'
            prepare.write_text('{}')
            candidate = {'prepare': proof.reference(self.root, prepare)}
            runs = {}
            for kind in ('check', 'test'):
                path = folder / (kind + '.json')
                path.write_text('{"fixture":"binding only, never executed"}')
                runs[kind] = proof.reference(self.root, path)
            record = {'runs': runs}
            carry = {'schema': 'airs-desktop/rust-test-build-carry/1',
                     'build': proof.reference(self.root, build_path), 'prepare': candidate['prepare'],
                     'native_rebuilt': False, 'reason': 'Synthetic test-only carry fixture.',
                     'changed_inputs': {proof.CHECK_TEST_PATH: {'before': before, 'after': after}},
                     'check_selection': runs['check'], 'tests': runs['test']}
            carry_path = folder / 'carry.json'

            def verify(changed=None):
                carry_path.write_text(json.dumps(carry))
                record['build_input_carry'] = proof.reference(self.root, carry_path)
                proof.rust_test_carry(self.root, record, build_path, build, candidate,
                                     changed if changed is not None else [proof.CHECK_TEST_PATH])
            verify()
            for changed in ([], ['crates/desktop-core/src/authority/setup_warning_domain.rs'],
                            [proof.CHECK_TEST_PATH, 'apps/desktop/src/application.tsx']):
                with self.subTest(changed=changed), self.assertRaises(ValueError): verify(changed)
            original = copy.deepcopy(carry)
            for change in ('candidate', 'build', 'delta', 'rebuilt', 'check', 'test'):
                carry.clear(); carry.update(copy.deepcopy(original))
                if change == 'candidate': carry['prepare'] = dict(carry['prepare'], sha256='another candidate')
                elif change == 'build': carry['build'] = dict(carry['build'], sha256='another build')
                elif change == 'delta': carry['changed_inputs'][proof.CHECK_TEST_PATH]['after'] = 'other edit'
                elif change == 'rebuilt': carry['native_rebuilt'] = True
                elif change == 'check': carry.pop('check_selection')
                else: carry['tests'] = dict(carry['tests'], sha256='stale test run')
                with self.subTest(change=change), self.assertRaises(ValueError): verify()
            carry.clear(); carry.update(original)
            record.pop('runs')
            with self.assertRaisesRegex(ValueError, 'current Check/Test bindings'): verify()
            record['runs'] = runs
            test.write_text('a subsequent test edit\n')
            with self.assertRaisesRegex(ValueError, 'reviewed Rust test-only change'): verify()


if __name__ == '__main__':
    unittest.main()
