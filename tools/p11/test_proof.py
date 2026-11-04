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


    def impact_fixture(self):
        bind_check, _, old_selection, _, _, before, _ = self.check_selection_fixture()
        folder = self.root / 'docs/planning/evidence/P11/impact-fixture'
        folder.mkdir()
        baseline_root = self.root / proof.D2_SOURCE
        for name in before['files']:
            target = baseline_root / name
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(self.root / name, target)
        baseline_check = self.root / 'docs/planning/evidence/P11/check-fixture/selection.json'
        old_inputs = old_selection['runner_inputs']
        names = ['rustc', 'cargo', 'rustc-toolchain', 'cargo-toolchain', 'node', 'pnpm']
        tools = [dict(name=name, version='same pinned fixture version', sha256='old-' + name)
                 for name in names]
        new_tools = copy.deepcopy(tools)
        for row in new_tools:
            if row['name'] in ('rustc', 'cargo'):
                row['sha256'] = 'new-shim-' + row['name']
        def write(name, value):
            path = folder / name
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(json.dumps(value))
            return proof.reference(self.root, path)
        def raw(kind, labels, inputs, identity, destination):
            commands = []
            for i, needed in enumerate(proof.command_plan(self.root, kind)):
                if labels is not None and needed['label'] not in labels:
                    continue
                log = folder / destination / (str(i) + '.log')
                log.parent.mkdir(parents=True, exist_ok=True)
                log.write_text('Synthetic validator fixture; never a product pass.\n')
                commands.append(dict(label=needed['label'], argv=needed['argv'], cwd='.',
                    status='passed', exit_code=0, note=None, log=log.name,
                    output_sha256=proof.sha(log), tests={'passed': 1, 'failed': 0}
                    if needed['expects_tests'] else None))
            result = dict(schema='airs-v2/run/1', subject='P11', kind=kind, status='passed',
                failure=None, inputs_before=inputs, inputs_after=inputs,
                commands=commands, toolchain=copy.deepcopy(identity), run_id='fixture-' + destination,
                started_utc='fixture-start', finished_utc='fixture-finish')
            return result, write(destination + '/run.json', result)
        old_test, old_test_ref = raw('test', None, old_inputs, tools, 'old-test')
        app = self.root / 'apps/desktop/src/v2/modal.tsx'
        app.parent.mkdir(parents=True, exist_ok=True)
        app.write_text('synthetic affected frontend only\n')
        after = {'files': proof.source_inventory(self.root), 'git_config': before['git_config']}
        inputs = dict(old_inputs, source=proof.inventory_source(after), dependencies='new-shim-deps')
        affected = {'check': list(proof.IMPACT_CHECKS), 'test': [
            row['label'] for row in proof.command_plan(self.root, 'test')
            if not row['label'].startswith('setup/') and row['argv'][:2] != ['cargo', 'test']]}
        fresh = {}; run_refs = {}
        for kind in ('check', 'test'):
            fresh[kind], run_refs[kind] = raw(kind, affected[kind], inputs, new_tools, 'new-' + kind)
        audit = dict(current_tools=copy.deepcopy(new_tools), comparison=[
            dict(name=a['name'], old_sha256=a['sha256'], new_sha256=b['sha256'])
            for a, b in zip(tools, new_tools)])
        origin = dict(source_inventory_sha256=proof.digest(before['files']), runner_inputs=old_inputs,
            execution_root=str(self.root), executable_sha256='synthetic-executable',
            bundle_sha256='synthetic-bundle', build_flags={'target': 'fixture-arm64'},
            conditions={'host': 'original-fixture-host', 'native': 'unqualified'},
            helper_closure={'synthetic-helper': 'hash'})
        origin_ref = write('origin.json', origin)
        record = dict(schema='airs-desktop/p11-impact-selection/1', runner_inputs=inputs,
            reason='Synthetic unchanged Rust closure and audited tool-shim transition.',
            baseline_source=write('before.json', before), baseline_candidate=origin_ref,
            baseline_check=proof.reference(self.root, baseline_check), baseline_test=old_test_ref,
            current_source=write('after.json', after),
            changed_paths=sorted(name for name in set(before['files']) | set(after['files'])
                if before['files'].get(name) != after['files'].get(name)),
            dependencies=proof.carry_dependencies(before['files']), affected=affected,
            current_runs=run_refs, tool_transition=write('audit.json', audit))
        selection_path = folder / 'selection.json'
        def bind():
            for kind in ('check', 'test'):
                record['current_runs'][kind] = write('new-' + kind + '/run.json', fresh[kind])
            record['tool_transition'] = write('audit.json', audit)
            record['current_source'] = write('after.json', after)
            write('selection.json', record)
        def verify():
            return proof.validate_impact_selection(self.root, selection_path, inputs)
        bind()
        return bind, verify, record, fresh, audit, after, inputs, origin_ref, write

    def test_impact_selection_requires_complete_current_frontend_and_original_bindings(self):
        bind, verify, record, fresh, audit, after, inputs, _, _ = self.impact_fixture()
        selected = verify()
        self.assertEqual(sum(r['disposition'] == 'carried_unaffected'
                             for r in selected['choices']['test']), 8)
        self.assertEqual(sum(r['disposition'] == 'executed'
                             for r in selected['choices']['test']), 12)
        original = copy.deepcopy(record)
        for change in ('missing-frontend', 'wrong-delta', 'tiny-deps'):
            record.clear(); record.update(copy.deepcopy(original))
            if change == 'missing-frontend': record['affected']['test'].pop()
            elif change == 'wrong-delta': record['changed_paths'] = []
            else: record['dependencies'] = record['dependencies'][:1]
            bind()
            with self.subTest(change=change), self.assertRaises(ValueError): verify()
        record.clear(); record.update(original)
        fresh['test']['commands'][0]['tests']['passed'] = 0
        bind()
        with self.assertRaisesRegex(ValueError, 'no passing tests'): verify()

    def test_impact_selection_rejects_changed_tool_or_dependency(self):
        bind, verify, record, fresh, audit, after, inputs, _, write = self.impact_fixture()
        original = copy.deepcopy(audit)
        audit['comparison'][2]['new_sha256'] = 'changed compiler'
        bind()
        with self.assertRaisesRegex(ValueError, 'actual executables'): verify()
        audit.clear(); audit.update(original)
        module = self.root / 'crates/desktop-core/src/authority/setup_warning_domain.rs'
        module.write_text('changed production Rust behavior\n')
        after['files'] = proof.source_inventory(self.root)
        inputs['source'] = proof.inventory_source(after)
        record['changed_paths'] = sorted(name for name in
            set(proof.read(proof.bound(self.root, record['baseline_source']))['files']) | set(after['files'])
            if proof.read(proof.bound(self.root, record['baseline_source']))['files'].get(name) != after['files'].get(name))
        bind()
        with self.assertRaisesRegex(ValueError, 'carried dependency membership/content'): verify()

    def test_selected_runner_rejects_unregistered_and_failed_partial_records(self):
        path, raw = self.run_fixture('test')
        labels = [row['label'] for row in proof.command_plan(self.root, 'test')
                  if not row['label'].startswith('setup/') and row['argv'][:2] != ['cargo', 'test']]
        raw['commands'] = [row for row in raw['commands'] if row['label'] in labels]
        path.write_text(json.dumps(raw))
        proof.validate_run(self.root, path, 'test', self.inputs, labels=labels)
        for selected in (labels[:-1], labels + [labels[-1]], ['unknown'], ['setup/1']):
            with self.subTest(labels=selected), self.assertRaises(ValueError):
                proof.validate_run(self.root, path, 'test', self.inputs, labels=selected)
        raw['commands'][0]['exit_code'] = 1
        path.write_text(json.dumps(raw))
        with self.assertRaisesRegex(ValueError, 'failed or was skipped'):
            proof.validate_run(self.root, path, 'test', self.inputs, labels=labels)


    def test_impact_generic_carry_survives_fresh_clone_without_target(self):
        from standing import ArchivedP11
        bind, verify, record, fresh, audit, after, inputs, origin_ref, write = self.impact_fixture()
        selected = verify()
        source_hash = proof.digest(after['files'])
        candidate = dict(selected['origin'], source_inventory_sha256=source_hash,
                         runner_inputs=inputs, executable_sha256='current-fixture-executable',
                         bundle_sha256='current-fixture-bundle', conditions={'host': 'new-fixture-vm'})
        candidate_ref = write('current-candidate.json', candidate)
        requirements, rows = [], []
        for kind in ('check', 'test'):
            for choice in selected['choices'][kind]:
                needed, actual, raw, path = (choice[k] for k in ('needed', 'actual', 'raw', 'path'))
                cid = kind + ':' + needed['label']
                carried = choice['disposition'] == 'carried_unaffected'
                requirement = dict(id=cid, kind='command', **needed, affected=not carried,
                                   must_execute=not carried, helper_paths=[])
                if carried: requirement['dependencies'] = selected['dependencies']
                requirements.append(requirement)
                result = dict(status='pass', exit_code=0, execution_id=raw['run_id'],
                    argv=actual['argv'], cwd=str(self.root), started_utc=raw['started_utc'],
                    finished_utc=raw['finished_utc'], required_ids=[cid],
                    log=proof.reference(self.root, path.parent / actual['log']),
                    candidate=origin_ref if carried and kind == 'test' else candidate_ref,
                    raw=proof.reference(self.root, path), evidence={})
                if kind == 'check':
                    result.update(original_runner_inputs=raw['inputs_before'], reason=choice['reason'])
                row = dict(id=cid, disposition=choice['disposition'],
                           result=write('results/' + cid.replace('/', '-') + '.json', result))
                if carried and kind == 'test':
                    deps = selected['dependencies']
                    proof_ref = write('impacts/' + cid.replace('/', '-') + '.json', {
                        'unchanged_dependencies': deps,
                        'current_source_inventory_sha256': source_hash,
                        'original_candidate_sha256': origin_ref['sha256'],
                        'file_bindings': {name: {'original_sha256': proof.sha(self.root / proof.D2_SOURCE / name),
                            'current_sha256': proof.sha(self.root / name),
                            'original_file': proof.reference(self.root, self.root / proof.D2_SOURCE / name)}
                            for name in deps}})
                    row['impact'] = dict(reason=record['reason'], dependencies=deps,
                        original_candidate_sha256=origin_ref['sha256'], proof=proof_ref)
                rows.append(row)
        native_ids = [r['id'] for chunk in proof.CHUNKS for r in
                      proof.read(self.root / 'docs/architecture/native-checks.json')['chunks'][chunk]]
        observation = write('observations.json', {'fixture': 'not a native product execution'})
        native_ref = write('native.json', {'schema': 'airs-v2/native-acceptance/1',
            'app': {'executable_sha256': candidate['executable_sha256']},
            'checks': [dict(id=cid, result='pass', observation='synthetic validator only',
                            evidence=[observation['path']]) for cid in native_ids]})
        for cid in native_ids:
            requirements.append(dict(id=cid, kind='native', argv=['fixture-native'],
                                     affected=True, must_execute=True))
            rows.append(dict(id=cid, disposition='executed', result=write('results/' + cid + '.json',
                dict(status='pass', exit_code=0, execution_id='fixture-native', argv=['fixture-native'],
                    cwd=str(self.root), started_utc='fixture-start', finished_utc='fixture-end',
                    required_ids=[cid], log=observation, candidate=candidate_ref, raw=native_ref,
                    evidence={observation['path']: observation['sha256']}))))
        plan = dict(schema='airs-desktop/required-coverage/1', subject='P11', requirements=requirements)
        qualified = dict(schema='airs-desktop/qualified-coverage/1',
            plan=write('plan.json', plan), candidate=candidate_ref,
            source_inventory_sha256=source_hash, coverage=rows,
            impact_selection=proof.reference(self.root,
                self.root / 'docs/planning/evidence/P11/impact-fixture/selection.json'),
            impact_source=record['current_source'],
            runtime_plan=write('runtime-plan.json', dict(plan, requirements=[
                r for r in requirements if not r['id'].startswith('check:')])))
        current_source = self.root / 'docs/planning/evidence/P11/accepted/source'
        for name in after['files']:
            destination = current_source / name
            destination.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(self.root / name, destination)
        context = write('context.json', {
            'schema': 'airs-desktop/historical-qualification-context/1',
            'sources': {source_hash: {'path': str(current_source.relative_to(self.root)),
                                     'seal': record['current_source']},
                        selected['origin']['source_inventory_sha256']: {
                            'path': str(proof.D2_SOURCE), 'seal': record['baseline_source']}},
            'applications': {}})
        expected = proof.verify_impact_coverage(self.root, qualified)
        self.assertEqual(expected['dispositions'], {'executed': 26, 'carried_unaffected': 15})
        with tempfile.TemporaryDirectory() as clone:
            clone = Path(clone).resolve()
            shutil.copytree(self.root, clone, dirs_exist_ok=True)
            self.assertFalse((clone / 'target').exists())
            history = ArchivedP11(clone, context)
            self.assertEqual(proof.verify_impact_coverage(clone, qualified, historical=history), expected)
            dependency = clone / proof.D2_SOURCE / selected['dependencies'][0]
            dependency.write_text('historical dependency drift\n')
            with self.assertRaisesRegex(ValueError, 'archive membership/content drift|D2 source archive'):
                proof.verify_impact_coverage(clone, qualified, historical=history)


if __name__ == '__main__':
    unittest.main()
