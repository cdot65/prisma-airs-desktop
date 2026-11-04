"""Synthetic native-consumer fixtures, never app or pixel acceptance results."""
from pathlib import Path
import copy
import importlib.util
import json
import shutil
import tempfile
import unittest
from unittest.mock import patch

_spec = importlib.util.spec_from_file_location('p11_native_validation', Path(__file__).with_name('native.py'))
n = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(n)


class NativeOriginsTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.root = Path(self.directory.name).resolve()
        real = Path(__file__).resolve().parents[2]
        manifest = self.root/'docs/architecture/native-checks.json'
        manifest.parent.mkdir(parents=True)
        shutil.copyfile(real/'docs/architecture/native-checks.json', manifest)
        self.scope = n.sha(manifest)
        patcher = patch.object(n, 'c46_helpers', return_value=['shared-helper'])
        patcher.start(); self.addCleanup(patcher.stop)
        self.candidate = dict(executable_sha256='fixture-app', bundle_sha256='fixture-bundle',
            helper_closure={'shared-helper': 'same', 'api-helper': 'new', 'resume-helper': 'new'},
            conditions={'platform': 'synthetic-VM', 'hostname': 'synthetic-vm'},
            build_flags={'target': 'fixture-arm64'})
        self.context = dict(copy.deepcopy(self.candidate),
            helper_closure={'shared-helper': 'same', 'api-helper': 'old'})
        self.prepare = dict(executable_sha256='fixture-app', bundle_sha256='fixture-bundle')
        self.prepare_ref = self.write('prepare.json', self.prepare)
        self.previous_path = self.root/'original/observations.json'
        self.resume_path = self.root/'continuation/observations.json'
        self.ids = sorted(set(n.inventory(self.root))-{'C46-N01'})
        self.raws = []
        for prior, path, ids in [(True, self.previous_path, ['C46-N01']), (False, self.resume_path, self.ids)]:
            helper = copy.deepcopy(self.context['helper_closure'] if prior else self.candidate['helper_closure'])
            raw = dict(schema='airs-desktop/p11-native-observations/1', subject='P11',
                execution_id=path.parent.name, performed_by='synthetic validator fixture only',
                argv=['python3', 'tools/native-acceptance/'+('acceptance_p11.py' if prior else 'acceptance_p11_resume.py')],
                executable_sha256='fixture-app', bundle_sha256='fixture-bundle', host='synthetic-VM',
                started_utc='2026-11-04T00:00:00Z', finished_utc='2026-11-04T00:00:01Z',
                helper_closure=helper, complete=not prior, status='failed' if prior else 'observed',
                exit_code=1 if prior else 0, errors=[{'message': 'later unrelated helper failed'}] if prior else [],
                binding={'prepare': self.prepare_ref, 'native_inventory_sha256': self.scope,
                         'helper_closure': helper, 'conditions': copy.deepcopy(self.candidate['conditions'])},
                checks=[], artifact_sha256={})
            for cid in ids:
                evidence = self.write(str(path.parent.relative_to(self.root))+'/'+cid+'.json',
                                      {'fixture': 'metadata consumer, not an app observation'})
                raw['artifact_sha256'][evidence['path']] = evidence['sha256']
                raw['checks'].append(dict(id=cid, facts={'fixture_completed_fact': True},
                                          evidence=[evidence['path']], visual=[]))
            self.raws.append(raw)
        image = self.root/'continuation/capture.png'
        image.write_bytes(b'synthetic image bytes, never visually accepted for the application')
        self.raws[1]['artifact_sha256'][str(image.relative_to(self.root))] = n.sha(image)
        self.raws[1]['checks'][0]['visual'] = [str(image.relative_to(self.root))]
        self.selection_path, self.acceptance_path = self.root/'native-selection.json', self.root/'native-acceptance.json'
        self.bind()

    def write(self, name, value):
        path = self.root/name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value))
        return n.reference(self.root, path)

    def bind(self):
        original = self.write('original/observations.json', self.raws[0])
        admission = dict(observations=original, completed_ids=['C46-N01'], remaining_ids=self.ids,
                         fixture_state_semantically_equal=True, certificate_generation_or_install_executed=False)
        self.raws[1]['continuation'] = admission
        receipt = self.write('continuation/resume-preconditions.json', admission)
        self.raws[1]['artifact_sha256'][receipt['path']] = receipt['sha256']
        continuation = self.write('continuation/observations.json', self.raws[1])
        review = dict(schema='airs-desktop/p11-visual-review/2', observations=[original, continuation],
            executable_sha256='fixture-app', bundle_sha256='fixture-bundle',
            performed_by='synthetic capture-review validator fixture', captures=[
                dict(path=image, assertion=row['id'], sha256=n.sha(self.root/image), accepted=True, findings=[])
                for raw in self.raws for row in raw['checks'] for image in row['visual']])
        self.selection = dict(schema='airs-desktop/p11-native-selection/1', prepare=self.prepare_ref,
            native_inventory_sha256=self.scope, source_context=self.write('context.json', self.context),
            visual_review=self.write('visual-review.json', review), origins=[
                dict(observations=original, ids=['C46-N01'], disposition='reused_exact', helper_paths=['shared-helper']),
                dict(observations=continuation, ids=self.ids, disposition='executed',
                     helper_paths=sorted(self.candidate['helper_closure']))])
        self.write('native-selection.json', self.selection)
        checks = []
        for raw, raw_ref in zip(self.raws, [original, continuation]):
            for row in raw['checks']:
                checks.append(dict(id=row['id'], result='pass', observation='synthetic validator fixture',
                    evidence=[*row['evidence'], *row['visual'], raw_ref['path'],
                              'native-selection.json', 'visual-review.json']))
        self.write('native-acceptance.json', dict(schema='airs-v2/native-acceptance/1', subject='P11',
            method='cua', app={'fixture': True, 'executable_sha256': 'fixture-app'}, checks=checks))

    def verify(self):
        return n.validate(self.root, self.acceptance_path, self.selection_path, self.candidate)

    def test_failed_process_can_contribute_only_its_completed_assertion(self):
        result = self.verify()
        self.assertEqual(result['C46-N01']['raw']['exit_code'], 1)
        self.assertEqual(result['C46-N01']['disposition'], 'reused_exact')
        self.assertEqual(len(result), 12)
        for change in ('false-fact', 'missing-row', 'failed-continuation', 'relabel-prior-pass'):
            original = copy.deepcopy(self.raws)
            if change == 'false-fact': self.raws[0]['checks'][0]['facts']['fixture_completed_fact'] = False
            elif change == 'missing-row': self.raws[1]['checks'].pop()
            elif change == 'failed-continuation': self.raws[1]['exit_code'] = 1
            else: self.raws[0]['exit_code'] = 0
            self.bind()
            with self.subTest(change=change), self.assertRaises(ValueError): self.verify()
            self.raws = original

    def test_helper_and_candidate_drift_cannot_reuse_completed_assertion(self):
        self.raws[0]['helper_closure']['shared-helper'] = 'changed'
        self.bind()
        with self.assertRaises(ValueError): self.verify()
        self.raws[0]['helper_closure']['shared-helper'] = 'same'
        self.bind()
        self.candidate['conditions']['hostname'] = 'another-vm'
        with self.assertRaisesRegex(ValueError, 'conditions changed'): self.verify()

    def test_capture_and_admission_proofs_are_bound_without_live_state_reads(self):
        self.verify()
        review = n.read(self.root/'visual-review.json')
        review['captures'][0]['accepted'] = False
        self.selection['visual_review'] = self.write('visual-review.json', review)
        self.write('native-selection.json', self.selection)
        with self.assertRaisesRegex(ValueError, 'capture review failed'): self.verify()
        self.bind()
        self.write('continuation/resume-preconditions.json', {'fixture_state_semantically_equal': False})
        with self.assertRaises(ValueError): self.verify()

    def test_manifest_consumer_survives_fresh_clone_without_app_or_current_host(self):
        self.verify()
        with tempfile.TemporaryDirectory() as clone:
            clone = Path(clone).resolve()
            shutil.copytree(self.root, clone, dirs_exist_ok=True)
            self.assertFalse((clone/'target').exists())
            result = n.validate(clone, clone/'native-acceptance.json',
                                clone/'native-selection.json', self.candidate)
            self.assertEqual(len(result), 12)


    def test_three_real_process_origins_retain_five_completed_rows(self):
        # Synthetic process manifests only; no app execution or pixel verdict.
        previous = copy.deepcopy(self.raws[0])
        middle = copy.deepcopy(self.raws[1])
        ids = {'C48-N01', 'C48-N02', 'C48-N03', 'C49-N01'}
        middle['checks'] = [row for row in middle['checks'] if row['id'] in ids]
        middle.update(complete=False, status='failed', exit_code=1, errors=[{'message': 'later Contents role failed'}])
        last = copy.deepcopy(self.raws[1])
        last['checks'] = [row for row in last['checks'] if row['id'] not in ids]
        self.candidate['helper_closure']['continue-helper'] = 'new'
        last['helper_closure'] = copy.deepcopy(self.candidate['helper_closure'])
        last['binding']['helper_closure'] = last['helper_closure']
        last['argv'][1] = 'tools/native-acceptance/acceptance_p11_continue.py'
        last['execution_id'] = 'last'
        old = self.root/'continuation'
        shutil.copytree(old, self.root/'last')
        last['artifact_sha256'] = {name.replace('continuation/', 'last/', 1): digest
                                   for name, digest in last['artifact_sha256'].items()}
        for row in last['checks']:
            for key in ('evidence', 'visual'):
                row[key] = [name.replace('continuation/', 'last/', 1) for name in row[key]]
        refs, completed = [], set()
        selection = copy.deepcopy(self.selection)
        selection['origins'] = []
        for i, (raw, name) in enumerate([(previous, 'original'), (middle, 'continuation'), (last, 'last')]):
            if i:
                admission = dict(origins=copy.deepcopy(refs), observations=refs[-1],
                    completed_ids=sorted(completed), remaining_ids=sorted(set(n.inventory(self.root))-completed),
                    fixture_state_semantically_equal=True, certificate_generation_or_install_executed=False)
                raw['continuation'] = admission
                receipt = self.write(name+'/resume-preconditions.json', admission)
                raw['artifact_sha256'][receipt['path']] = receipt['sha256']
            ref = self.write(name+'/observations.json', raw)
            refs.append(ref)
            completed.update(row['id'] for row in raw['checks'])
            selection['origins'].append(dict(observations=ref,
                ids=sorted(row['id'] for row in raw['checks']),
                disposition='executed' if i == 2 else 'reused_exact',
                helper_paths=['shared-helper'] if i == 0 else sorted(raw['helper_closure'])))
        selected = n.origins(self.root, selection, self.candidate)
        self.assertEqual(len(selected), 12)
        self.assertEqual(sum(row['disposition'] == 'reused_exact' for row in selected.values()), 5)
        self.assertEqual(selected['C48-N01']['raw']['exit_code'], 1)
        self.assertEqual(selected['C49-N01']['raw']['execution_id'], 'continuation')
        selection['origins'][1]['ids'].append('C46-N01')
        with self.assertRaisesRegex(ValueError, 'membership'): n.origins(self.root, selection, self.candidate)



class ResumeAdmissionTests(unittest.TestCase):
    def test_state_artifact_helper_and_process_drift_fail_before_ui(self):
        import sys
        folder = Path(__file__).resolve().parents[2]/'tools/native-acceptance'
        sys.path.insert(0, str(folder))
        spec = importlib.util.spec_from_file_location('p11_resume_admission_test', folder/'acceptance_p11_resume.py')
        resume = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(resume)
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary).resolve()
            work = root/'airs-p11-owned-fixture'; work.mkdir()
            original = root/'original'; original.mkdir()
            (root/'docs/architecture').mkdir(parents=True)
            (root/'docs/architecture/native-checks.json').write_text('{}')
            (root/'shared.py').write_text('synthetic helper')
            (root/'prepare.json').write_text('{}')
            state = {'fixture': 'captured state, no live data'}
            (original/'fixture-state-final.json').write_text(json.dumps(state))
            (root/'state.json').write_text(json.dumps(state))
            raw = dict(schema='airs-desktop/p11-native-observations/1', subject='P11',
                status='failed', exit_code=1, complete=False, host='synthetic-platform',
                checks=[{'id': 'C46-N01', 'facts': {'synthetic_completed_fact': True}}],
                helper_closure={'shared.py': n.sha(root/'shared.py')},
                binding={'prepare': {'sha256': n.sha(root/'prepare.json')},
                    'native_inventory_sha256': n.sha(root/'docs/architecture/native-checks.json')},
                artifact_sha256={'original/fixture-state-final.json': n.sha(original/'fixture-state-final.json')})
            path = original/'observations.json'
            def admit():
                path.write_text(json.dumps(raw))
                return resume.inspect_previous(path, root/'prepare.json', work, root/'state.json')
            with patch.object(resume, 'ROOT', root), patch.object(resume, 'relevant_c46', return_value=['shared.py']), \
                 patch.object(resume.platform, 'platform', return_value='synthetic-platform'), \
                 patch.object(resume.tempfile, 'gettempdir', return_value=str(root)):
                admit()
                (root/'state.json').write_text('{"fixture":"changed"}')
                with self.assertRaisesRegex(RuntimeError, 'state changed'): admit()
                (root/'state.json').write_text(json.dumps(state))
                (root/'shared.py').write_text('changed helper')
                with self.assertRaisesRegex(RuntimeError, 'helper changed'): admit()
                (root/'shared.py').write_text('synthetic helper')
                raw['exit_code'] = 0
                with self.assertRaisesRegex(RuntimeError, 'failed first-unfinished'): admit()
                raw['exit_code'] = 1
                (original/'fixture-state-final.json').write_text('{"fixture":"altered original"}')
                with self.assertRaisesRegex(RuntimeError, 'artifact changed'): admit()



if __name__ == '__main__':
    unittest.main()
