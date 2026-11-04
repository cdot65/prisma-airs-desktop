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
        self.root = Path(self.directory.name)
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


if __name__ == '__main__':
    unittest.main()
