"""Synthetic source/stage scope tests; no application or UI execution."""
from pathlib import Path
import copy,json,tempfile,unittest
from unittest.mock import patch
from qualification import sha
from candidate import digest
import continuation


class ContinuationTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup);self.root=Path(self.temp.name);self.base=self.root/'baseline';self.folder=self.base/'continuation-01';self.folder.mkdir(parents=True)
        name='apps/desktop/src-tauri/tests/permission_manifest.rs';source=self.root/name;source.parent.mkdir(parents=True);source.write_text('current synthetic identity assertions')
        self.before={name:{'sha256':'old'},'apps/desktop/src-tauri/src/lib.rs':{'sha256':'unchanged'}};self.after=copy.deepcopy(self.before);self.after[name]['sha256']='new'
        self.write(self.base/'source-seal.json',{'files':self.before});self.write(self.folder/'source-seal.json',{'files':self.after});self.write(self.base/'candidate.json',{'source_inventory_sha256':digest(self.before)})
        log=self.folder/'focused.log';log.write_text('actual synthetic test result')
        self.marker=dict(schema='airs-desktop/c45-test-only-continuation/1',affected_stage='command-C45-host-2',source_seal_sha256=sha(self.folder/'source-seal.json'),original_candidate_sha256=sha(self.base/'candidate.json'),declared_changed_paths=[name],retained_completed_stages=[],focused_checks=[dict(log=str(log.relative_to(self.root)),log_sha256=sha(log),exit_code=0,source_sha256=sha(source))])
        self.write(self.base/'continuation-01.json',self.marker)

    def write(self,path,value):path.write_text(json.dumps(value))
    def verify(self):
        with patch('continuation.source_inventory',return_value=self.after):return continuation.verify(self.root,self.base)

    def test_test_only_source_delta_and_affected_target_are_explicit(self):self.assertEqual(self.verify()['affected_stage'],'command-C45-host-2')

    def test_application_source_cannot_change_under_test_only_acceptance(self):
        name='apps/desktop/src-tauri/src/lib.rs';self.after[name]['sha256']='changed';self.write(self.folder/'source-seal.json',{'files':self.after});self.marker.update(source_seal_sha256=sha(self.folder/'source-seal.json'),declared_changed_paths=sorted(self.after));self.write(self.base/'continuation-01.json',self.marker)
        with self.assertRaisesRegex(ValueError,'application source'):self.verify()

    def test_failed_affected_stage_cannot_be_retained(self):
        self.marker['retained_completed_stages']=['command-C45-host-2'];self.write(self.base/'continuation-01.json',self.marker)
        with self.assertRaisesRegex(ValueError,'failed/affected'):self.verify()

    def test_focused_test_proof_cannot_drift(self):
        (self.folder/'focused.log').write_text('edited')
        with self.assertRaisesRegex(ValueError,'focused check'):self.verify()




class NativeContinuationTests(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup);self.root=Path(self.temp.name);self.base=self.root/'baseline';self.folder=self.base/'continuation-02';self.folder.mkdir(parents=True)
        self.driver='tools/native-acceptance/acceptance_c36.py';self.changed='tools/native-acceptance/acceptance_c32_current.py'
        for name,text in [(self.driver,'unchanged native assertions'),(self.changed,'canonical project path')]:
            p=self.root/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_text(text)
        self.before={self.driver:{'sha256':sha(self.root/self.driver)},self.changed:{'sha256':'old-alias'}};self.after=copy.deepcopy(self.before);self.after[self.changed]['sha256']=sha(self.root/self.changed)
        def write(name,value):
            p=self.root/name;p.parent.mkdir(parents=True,exist_ok=True);p.write_text(json.dumps(value));return dict(path=name,sha256=sha(p))
        self.write=write;before_ref=write('before.json',{'files':self.before});write('baseline/continuation-02/source-seal.json',{'files':self.after})
        candidate=write('origin-candidate.json',dict(source_inventory_sha256=digest(self.before),helper_closure={self.driver:sha(self.root/self.driver)}))
        stage=self.root/'old-native-stage';stage.mkdir()
        for name in ['stdout.log','stderr.log','raw.json']:(stage/name).write_text('actual synthetic native output')
        state=dict(exit_code=0,argv=['driver'],source_fingerprint=digest(self.before),candidate_sha256=candidate['sha256'],stdout_sha256=sha(stage/'stdout.log'),stderr_sha256=sha(stage/'stderr.log'),artifacts={'old-native-stage/raw.json':sha(stage/'raw.json')})
        completed=write('old-native-stage/completed.json',state);log=self.folder/'focused.log';log.write_text('Ran 2 tests\nOK\n')
        self.marker=dict(schema='airs-desktop/c45-native-helper-continuation/1',affected_stage='native-c32',source_before=before_ref,source_seal_sha256=sha(self.folder/'source-seal.json'),declared_changed_paths=[self.changed],retained_records={'native-c36':dict(folder='old-native-stage',completed=completed,candidate=candidate,source=before_ref,disposition='carried_unaffected',dependencies=[self.driver])},focused_check=dict(exit_code=0,log=str(log.relative_to(self.root)),log_sha256=sha(log),source_sha256=sha(self.root/self.changed)))
        write('baseline/continuation-02.json',self.marker)

    def verify(self):
        with patch('continuation.source_inventory',return_value=self.after):return continuation.verify(self.root,self.base)

    def test_unchanged_native_driver_retains_original_source_and_result(self):self.assertEqual(self.verify()['retained_records']['native-c36']['disposition'],'carried_unaffected')

    def test_changed_runtime_helper_cannot_be_carried_unaffected(self):
        (self.root/self.driver).write_text('changed native assertions')
        with self.assertRaisesRegex(ValueError,'assertion/helper changed'):self.verify()

    def test_failed_c32_cannot_be_retained(self):
        self.marker['retained_records']['native-c32']=self.marker['retained_records'].pop('native-c36');self.write('baseline/continuation-02.json',self.marker)
        with self.assertRaisesRegex(ValueError,'failed native driver'):self.verify()


class AdapterInvocationTests(unittest.TestCase):
    def test_current_plan_matches_actual_retained_g11_without_running_it(self):
        import seal
        result=seal.verify_historical_invocations(Path(__file__).resolve().parents[2])
        self.assertEqual(result['status'],'pass')
        self.assertIn(('c31',True),result['queue'])
        self.assertIn(('c39',False),result['queue'])

    def test_omitted_c31_adapter_is_rejected_before_any_driver(self):
        import seal
        with patch.object(seal,'LEGACY',seal.LEGACY-{'c31'}):
            with self.assertRaisesRegex(ValueError,'differs from actual retained G11'):
                seal.verify_historical_invocations(Path(__file__).resolve().parents[2])


if __name__=='__main__':unittest.main()
