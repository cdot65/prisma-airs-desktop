"""Synthetic exact-artifact/source-delta proof tests, no app or UI execution."""
from pathlib import Path
import copy,json,platform,tempfile,unittest
from unittest.mock import patch
from candidate import verify_build_reuse,bundle_hash,digest
from qualification import sha


class BuildReuseTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup);self.root=Path(self.temp.name)
        executable=self.root/'fixture.app/Contents/MacOS/prisma-airs-desktop';executable.parent.mkdir(parents=True);executable.write_text('same actual synthetic executable')
        self.bundle=executable.parents[2]
        self.before={'apps/source.rs':{'sha256':'unchanged'},'tools/native-acceptance/acceptance_c45.py':{'sha256':'old-selector'}}
        self.after=copy.deepcopy(self.before);self.after['tools/native-acceptance/acceptance_c45.py']['sha256']='corrected-selector'
        prepared=dict(bundle='fixture.app',executable_sha256=sha(executable),bundle_sha256=bundle_hash(self.bundle))
        self.prepared=self.write('prepared.json',prepared)
        self.original=dict(source_inventory_sha256=digest(self.before),runner_inputs=dict(dependencies='deps',build_flags='flags'),conditions=dict(platform='VM',hostname='worf'),prepare=self.prepared,executable_sha256=prepared['executable_sha256'],bundle_sha256=prepared['bundle_sha256'])
        self.current=dict(source='new-source',dependencies='deps',build_flags='flags')
        stage=self.root/'old-stage';stage.mkdir();(stage/'stdout.log').write_text('actual synthetic build output');(stage/'stderr.log').write_text('')
        state=dict(exit_code=0,argv=['actual-build'],source_fingerprint=digest(self.before),stdout_sha256=sha(stage/'stdout.log'),stderr_sha256=sha(stage/'stderr.log'),artifacts={'prepared.json':self.prepared['sha256']})
        self.record=dict(schema='airs-desktop/c45-helper-only-build-reuse/1',original_candidate=self.write('old-candidate.json',self.original),original_source=self.write('before.json',{'files':self.before}),current_source=self.write('after.json',{'files':self.after}),original_build_stage=self.write('old-stage/completed.json',state),declared_changed_paths=['tools/native-acceptance/acceptance_c45.py'])

    def write(self,name,value):
        p=self.root/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(value));return dict(path=name,sha256=sha(p))

    def verify(self):
        with patch('candidate.source_inventory',return_value=self.after):return verify_build_reuse(self.root,self.record,self.current,'VM','worf')

    def test_same_app_full_bundle_and_declared_helper_delta_reuses_actual_build(self):
        result=self.verify();self.assertFalse(result['new_build_executed']);self.assertEqual(result['executable_sha256'],self.original['executable_sha256'])

    def test_application_source_change_cannot_be_declared_helper_only(self):
        self.after['apps/source.rs']['sha256']='changed';self.record['current_source']=self.write('after.json',{'files':self.after});self.record['declared_changed_paths'].append('apps/source.rs');self.record['declared_changed_paths'].sort()
        with self.assertRaisesRegex(ValueError,'application-affecting'):self.verify()

    def test_complete_bundle_change_rejects_reuse_even_when_executable_same(self):
        (self.bundle/'new-resource').write_text('changed resource')
        with self.assertRaisesRegex(ValueError,'complete bundle'):self.verify()

    def test_dependencies_and_execution_conditions_must_match(self):
        self.current['dependencies']='changed'
        with self.assertRaisesRegex(ValueError,'dependency/toolchain'):self.verify()
        self.current['dependencies']='deps';self.original['conditions']['hostname']='other';self.record['original_candidate']=self.write('old-candidate.json',self.original)
        with self.assertRaisesRegex(ValueError,'conditions'):self.verify()

    def test_original_build_logs_cannot_be_edited_and_rebound(self):
        (self.root/'old-stage/stdout.log').write_text('edited')
        with self.assertRaisesRegex(ValueError,'log drift'):self.verify()


if __name__=='__main__':unittest.main()
