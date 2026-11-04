"""Adversarial proof-consumer fixtures; these are not application test results."""
from pathlib import Path
import copy
import json
import shutil
import subprocess
import tempfile
import unittest

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


if __name__ == '__main__':
    unittest.main()
