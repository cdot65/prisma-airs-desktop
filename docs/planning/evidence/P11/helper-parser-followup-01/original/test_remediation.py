"""Adversarial remediation-validator fixtures, never application qualification evidence."""
from pathlib import Path
from io import BytesIO
import copy
import hashlib
import json
import tarfile
import tempfile
import unittest
from unittest.mock import patch

import remediation


class RemediationTests(unittest.TestCase):
    def setUp(self):
        directory = tempfile.TemporaryDirectory()
        self.addCleanup(directory.cleanup)
        self.root = Path(directory.name).resolve()
        self.app = 'apps/desktop/src/v2/workbench/api-client.tsx'
        self.rust = 'crates/desktop-core/tests/harness_tools/main.rs'
        self.app_pair = ('fixture-old-app', 'fixture-new-app')
        self.rust_pair = ('fixture-old-test', 'fixture-new-test')
        app_patch = patch.object(remediation, 'APP_DELTA', {self.app: self.app_pair})
        rust_patch = patch.object(remediation, 'TEST_DELTA', {self.rust: self.rust_pair})
        app_patch.start(); rust_patch.start()
        self.addCleanup(app_patch.stop); self.addCleanup(rust_patch.stop)

    def put(self, name, data):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        return {'path': name, 'sha256': hashlib.sha256(data).hexdigest()}

    def json(self, name, value):
        return self.put(name, (json.dumps(value, sort_keys=True) + '\n').encode())

    def test_delta_requires_every_exact_reviewed_pair_and_rejects_other_production(self):
        before = {self.app: {'sha256': self.app_pair[0]}, self.rust: {'sha256': self.rust_pair[0]}}
        after = {self.app: {'sha256': self.app_pair[1]}, self.rust: {'sha256': self.rust_pair[1]}}
        # A declared verifier edit is allowed alongside both exact, mandatory reviewed fixes.
        before['tools/p11/remediation.py'] = {'sha256': 'old-verifier'}
        after['tools/p11/remediation.py'] = {'sha256': 'new-verifier'}
        declared = sorted(after)
        self.assertEqual(remediation.delta(before, after, declared), declared)
        for change in ('undeclared', 'unrelated', 'altered', 'omitted', 'unfrozen'):
            b, a, names = copy.deepcopy(before), copy.deepcopy(after), declared[:]
            if change == 'undeclared': names.pop()
            elif change == 'unrelated':
                a['apps/desktop/src/app.tsx'] = {'sha256': 'unreviewed'}
                names = sorted(a)
            elif change == 'altered': a[self.app]['sha256'] = 'different-app-fix'
            elif change == 'omitted':
                a[self.rust] = b[self.rust]
                names.remove(self.rust)
            else:
                with patch.object(remediation, 'APP_DELTA', {}), self.assertRaises(ValueError):
                    remediation.delta(b, a, names)
                continue
            with self.subTest(change=change), self.assertRaises(ValueError):
                remediation.delta(b, a, names)

    def test_dependencies_include_all_resources_and_exclude_only_pinned_scope_changes(self):
        retained = {'resources/harness.json', 'resources/icons/another.svg', 'resources/fixtures/nested.bin',
                    'crates/desktop-core/src/lib.rs', 'crates/xtask/src/main.rs',
                    'packages/desktop-contracts/src/generated/Intent.ts', 'docs/architecture/checks.json',
                    '.cargo/config.toml', 'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml',
                    'pnpm-lock.yaml', 'pnpm-workspace.yaml', 'package.json', 'crates/xtask/Cargo.toml'}
        frontend = 'apps/desktop/src/v2/store.ts'
        helper = 'tools/native-acceptance/owned-fixture.py'
        files = {n: {'sha256': 'fixture'} for n in retained | {frontend, helper, self.app, self.rust, 'README.md'}}
        core = remediation.dependencies(files, 'test:C46/core/1', [helper])
        self.assertEqual(set(core), retained | {helper})
        ui = remediation.dependencies(files, 'test:C48/frontend/1', [helper])
        self.assertEqual(set(ui), retained | {frontend, helper})
        self.assertEqual(ui, sorted(ui))
        self.assertNotIn(self.app, ui)
        self.assertNotIn(self.rust, ui)
        # An unpinned sibling production file cannot disappear from an unaffected native closure.
        files['apps/desktop/src/another.tsx'] = {'sha256': 'must-be-compared'}
        self.assertIn('apps/desktop/src/another.tsx', remediation.dependencies(files, 'C49-N01'))

    def bundle_fixture(self, executable=b'original-executable'):
        archive = self.root / 'fixture-app.tar'
        with tarfile.open(archive, 'w') as stream:
            directory = tarfile.TarInfo('Fixture.app'); directory.type = tarfile.DIRTYPE
            stream.addfile(directory)
            item = tarfile.TarInfo('Fixture.app/Contents/MacOS/prisma-airs-desktop')
            item.size = len(executable); item.mode = 0o755
            stream.addfile(item, BytesIO(executable))
        exe = hashlib.sha256(executable).hexdigest()
        bundle = hashlib.sha256(('Contents/MacOS/prisma-airs-desktop\0' + '100755\0' + exe + '\n').encode()).hexdigest()
        return {'path': archive.name, 'sha256': remediation.sha(archive), 'size': archive.stat().st_size}, exe, bundle

    def test_preserved_bundle_rejects_changed_tar_even_when_rebound_and_wrong_identity(self):
        archive, exe, bundle = self.bundle_fixture()
        original = {'prepare': {'path': 'original-prepare.json', 'sha256': 'fixture-prepare'},
                    'executable_sha256': exe, 'bundle_sha256': bundle}
        record = {'schema': 'airs-desktop/p11-preserved-candidate/1',
                  'old_qualification': {'path': remediation.BASELINE}, 'original_prepare': original['prepare'],
                  'archive': archive, 'executable_sha256': exe, 'bundle_sha256': bundle}
        reference = self.json('preserved.json', record)
        remediation.preserved_bundle(self.root, reference, original)
        # Rewriting only tar bytes must fail the immutable reference first.
        (self.root / archive['path']).write_bytes(b'changed archive bytes')
        with self.assertRaises(ValueError): remediation.preserved_bundle(self.root, reference, original)
        # Rehashing the changed archive and manifest cannot substitute a different executable.
        changed_archive, _, _ = self.bundle_fixture(b'altered-executable')
        changed = dict(record, archive=changed_archive)
        changed_ref = self.json('rebound.json', changed)
        with self.assertRaisesRegex(ValueError, 'application bytes differ'):
            remediation.preserved_bundle(self.root, changed_ref, original)
        restored_archive, _, _ = self.bundle_fixture()
        reference = self.json('preserved.json', dict(record, archive=restored_archive))
        with self.assertRaisesRegex(ValueError, 'application bytes differ'):
            remediation.preserved_bundle(self.root, reference, dict(original, bundle_sha256='foreign-bundle'))
        with self.assertRaisesRegex(ValueError, 'candidate source differs'):
            remediation.preserved_bundle(self.root, reference, dict(original, prepare={'path': 'foreign-prepare'}))

    def test_source_selection_requires_original_producers_and_bounded_source_hashes(self):
        # These five synthetic selector records exercise the validator, not real application checks.
        app_input = self.put('inputs/final-app.tsx', b'validator fixture final app source')
        test_name = 'apps/desktop/src/v2/workbench/api-client.test.tsx'
        test_input = self.put('inputs/api.test.tsx', b'validator fixture tests')
        files = {self.app: {'sha256': app_input['sha256']}, test_name: {'sha256': test_input['sha256']}}
        selected = []
        producers = [[], []]
        coverage = [('C48/recovery-final02', 4), ('C48/typecheck-final02', None),
                    ('C48/controllers-final', 20), ('C48/api-prior', 42), ('C48/shell-and-cases-prior', 45)]
        for index, (cid, passed) in enumerate(coverage):
            log = self.put('logs/selector%d.log' % index, ('validator fixture only: ' + cid).encode())
            command = {'argv': ['fixture-validator', cid], 'cwd': '/validator/temporary-root',
                       'exit_code': 0, 'passed': passed}
            original = dict(command, log='original-private/selector%d.log' % index, log_sha256=log['sha256'])
            producers[0 if index < 2 else 1].append(original)
            hashes = {self.app: app_input['sha256']}
            inputs = {self.app: app_input}
            if index >= 2:
                hashes[test_name] = test_input['sha256']; inputs[test_name] = test_input
            selected.append(dict(command, id=cid, log=log, source_hashes=hashes, source_inputs=inputs))
        producer_refs = [self.json('producer%d.json' % index, {'final_checks': rows,
                         'meaning': 'pure validator fixtures, no application execution'})
                         for index, rows in enumerate(producers)]
        supplement = {'schema': 'airs-desktop/p11-c48-source-supplement/1',
                      'source_files': {self.app: app_input['sha256']},
                      'producer_receipts': producer_refs, 'checks': selected}
        def consume(value):
            reference = self.json('source-selection.json', value)
            return remediation.source_qa(self.root, reference, files)
        with patch.object(remediation, 'APP_DELTA', {self.app: ('fixture-before', app_input['sha256'])}):
            self.assertEqual(len(consume(supplement)['checks']), 5)
            for change in ('omitted', 'forged-command', 'forged-log', 'unknown-prior-source'):
                altered = copy.deepcopy(supplement)
                if change == 'omitted': altered['checks'].pop()
                elif change == 'forged-command': altered['checks'][0]['argv'].append('not-in-producer')
                elif change == 'forged-log':
                    altered['checks'][0]['log'] = self.put('logs/invented.log', b'not original producer output')
                else:
                    unknown = self.put('inputs/unreviewed-app.tsx', b'unknown earlier production change')
                    altered['checks'][2]['source_hashes'][self.app] = unknown['sha256']
                    altered['checks'][2]['source_inputs'][self.app] = unknown
                with self.subTest(change=change), self.assertRaises(ValueError):
                    consume(altered)

    def test_versioned_build_preserves_canonical_output_and_exact_fixture_recipe(self):
        # Preparation metadata and commands are validator fixtures; no builder is executed.
        canonical = 'docs/planning/evidence/P11/native-prepare.json'
        versioned = 'docs/planning/evidence/P11/native-build-05-vm/native-prepare.json'
        prepared = {'commands': [['fixture-build-recipe']], 'target': 'fixture-arm64-target',
                    'features': ['fixture'], 'subject': 'P11'}
        old_ref = self.json(canonical, prepared)
        new_ref = self.json(versioned, prepared)
        candidate = {'prepare': new_ref}
        argv = ['cargo', 'run', '--locked', '-p', 'xtask', '--example', 'p11_native_candidate', '--', '--out', versioned]
        build = {'steps': [{'argv': argv}], 'application_inputs_before': {'app': 'same-fixture-input'},
                 'application_inputs_after': {'app': 'same-fixture-input'}}
        remediation.versioned_build(self.root, build, candidate)
        offline = copy.deepcopy(build); offline['steps'][0]['argv'].insert(2, '--offline')
        remediation.versioned_build(self.root, offline, candidate)
        for change in ('canonical-output', 'old-version', 'changed-recipe', 'changed-after', 'different-command'):
            current, record = copy.deepcopy(candidate), copy.deepcopy(build)
            if change == 'canonical-output':
                current['prepare'] = old_ref
                record['steps'][0]['argv'][-1] = canonical
            elif change == 'old-version':
                current['prepare'] = dict(new_ref, path=versioned.replace('-05-', '-04-'))
                record['steps'][0]['argv'][-1] = current['prepare']['path']
            elif change == 'changed-recipe':
                current['prepare'] = self.json(versioned, dict(prepared, commands=[['different-fixture-recipe']]))
            elif change == 'changed-after': record['application_inputs_after']['app'] = 'different-input'
            else: record['steps'][0]['argv'] = ['echo', 'a-build-was-not-executed']
            with self.subTest(change=change), self.assertRaises(ValueError):
                remediation.versioned_build(self.root, record, current)
            self.json(versioned, prepared)

    def native_fixture(self, kind):
        source = self.put('tools/native-acceptance/fixture.py', b'# validator fixture only\n')
        execution = self.json('execution.json', {'meaning': 'pure validator fixture, no actual UI execution'})
        candidate = {'prepare': {'path': 'prepare.json', 'sha256': 'fixture-prepare'},
                     'executable_sha256': 'fixture-executable', 'bundle_sha256': 'fixture-bundle',
                     'execution_root': '/fixture/vm/root', 'conditions': {'hostname': 'fixture-vm'}}
        baseline = {'path': remediation.BASELINE, 'sha256': 'fixture-baseline'}
        facts = (remediation.SCROLL_FACTS if kind == 'scroll' else
                 {'missing_restore_warning_visible', 'recovery_survives_home_back', 'retry_is_explicit',
                  'retry_recovers_when_root_restored', 'intentional_close_stays_closed'})
        raw = {'facts': dict.fromkeys(facts, True), 'exit_code': 0, 'argv': ['fixture-only'],
               'cwd': candidate['execution_root'], 'started_utc': '2026-01-01T00:00:00Z', 'finished_utc': '2026-01-01T00:00:01Z',
               'source_sha256': source['sha256'], 'executable_sha256': candidate['executable_sha256'],
               'bundle_sha256': candidate['bundle_sha256'], 'conditions': candidate['conditions'],
               'helper_closure': {source['path']: source['sha256']}}
        captures = [dict(self.put('capture%d.png' % n, b'validator fixture pixels'),
                         visually_reviewed=True, accepted=True, findings=[]) for n in range(2)]
        supplement = {'schema': ('airs-desktop/p11-drawer-scroll-supplement/1' if kind == 'scroll'
                                 else 'airs-desktop/p11-recovery-supplement/1'),
                      'baseline_qualification': baseline, 'facts': raw['facts'], 'execution': execution,
                      'source': source, 'helper_closure': {source['path']: source['sha256']},
                      'executable_sha256': candidate['executable_sha256'], 'bundle_sha256': candidate['bundle_sha256'],
                      'captures': captures, 'prepare': candidate['prepare'],
                      'retained_result': {'path': 'original-result.json', 'sha256': 'fixture-result'},
                      'retained_reason': 'Preserve original first-use facts; fixture only.'}
        if kind == 'scroll':
            supplement['assertion'] = 'C47-N02'
            raw.update(baseline_qualification=baseline, fixture_state_unchanged=True,
                       fixture_modes_unchanged=True, trust_roots_unchanged=True,
                       state_before_sha256='same-state', state_after_sha256='same-state',
                       before_anchor={'pos': [1, 2], 'size': [100, 200]},
                       after_anchor={'pos': [1, 2], 'size': [100, 200]},
                       before_first_call={'pos': [10, 200]}, after_first_call={'pos': [10, 20]},
                       before_lower_call={'pos': [10, 800]}, after_lower_call={'pos': [10, 100]})
        return supplement, raw, candidate, baseline, {source['path']: {'sha256': source['sha256']}}

    def test_native_supplements_reject_foreign_artifacts_vm_and_missing_facts(self):
        for kind in ('scroll', 'recovery'):
            supplement, raw, candidate, baseline, files = self.native_fixture(kind)
            def consume(s, r):
                s = copy.deepcopy(s); s['raw'] = self.json(kind + '-raw.json', r)
                reference = self.json(kind + '.json', s)
                if kind == 'scroll': return remediation.scroll(self.root, reference, baseline, candidate, files)
                return remediation.recovery(self.root, reference, baseline, candidate, supplement['retained_result'], files)
            consume(supplement, raw)
            for change in ('artifact', 'missing-fact', 'unreviewed-capture', 'helper', 'reversed-time') + (('vm',) if kind == 'recovery' else ()):
                s, r = copy.deepcopy(supplement), copy.deepcopy(raw)
                if change == 'artifact':
                    s['executable_sha256'] = r['executable_sha256'] = 'foreign-executable'
                elif change == 'missing-fact':
                    name = next(iter(s['facts'])); s['facts'].pop(name); r['facts'].pop(name)
                elif change == 'unreviewed-capture': s['captures'][0]['visually_reviewed'] = False
                elif change == 'helper': s['helper_closure'][next(iter(s['helper_closure']))] = 'foreign-helper'
                elif change == 'reversed-time': r['finished_utc'] = '2025-12-31T23:59:59Z'
                else: r['conditions'] = {'hostname': 'different-vm'}
                with self.subTest(kind=kind, change=change), self.assertRaises(ValueError):
                    consume(s, r)


if __name__ == '__main__':
    unittest.main()
