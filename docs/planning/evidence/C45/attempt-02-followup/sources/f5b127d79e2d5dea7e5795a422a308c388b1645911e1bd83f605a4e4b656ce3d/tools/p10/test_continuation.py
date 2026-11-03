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


if __name__=='__main__':unittest.main()
