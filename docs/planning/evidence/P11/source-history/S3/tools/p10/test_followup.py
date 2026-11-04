"""Causal followup guard negatives; no native UI, services or real credentials."""
from pathlib import Path
import copy,json,tempfile,unittest
from qualification import sha
from followup import source_delta,raw_checks,COMMANDS


class FollowupTests(unittest.TestCase):
    def test_named_synthetic_tests_may_change(self):
        name='crates/xtask/tests/live_run/common.rs';self.assertEqual(source_delta({name:'old'},{name:'new'},[name]),[name])

    def test_production_or_unlisted_source_cannot_be_carried(self):
        for name in ['apps/desktop/src-tauri/src/lib.rs','crates/xtask/src/live.rs']:
            with self.assertRaisesRegex(ValueError,'application-affecting'):source_delta({name:'old'},{name:'new'},[name])

    def test_undeclared_changed_test_is_rejected(self):
        name='crates/xtask/tests/live_run/common.rs'
        with self.assertRaisesRegex(ValueError,'undeclared'):source_delta({name:'old'},{name:'new'},[])

    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup);self.root=Path(self.temp.name);self.inputs=dict(source='current',dependencies='same',build_flags='same');self.commands=[]
        for index,(label,argv,tests) in enumerate(COMMANDS):
            log=self.root/(str(index)+'.log');log.write_text('synthetic retained passing output')
            self.commands.append(dict(label=label,argv=argv,cwd='.',status='passed',exit_code=0,note=None,log=log.name,output_sha256=sha(log),tests=dict(passed=72,failed=0) if tests else None))
        self.raw=dict(schema='airs-v2/run/1',subject='C45',run_id='synthetic',status='passed',failure=None,inputs_before=self.inputs,inputs_after=self.inputs,commands=copy.deepcopy(self.commands))

    def verify(self):
        path=self.root/'run.json';path.write_text(json.dumps(self.raw));return raw_checks(self.root,dict(path='run.json',sha256=sha(path)),self.inputs)

    def test_actual_focused_test_lint_format_command_membership(self):self.assertEqual(self.verify()['commands'],3)

    def test_test_target_cannot_be_omitted_or_replaced(self):
        self.raw['commands'].pop()
        with self.assertRaisesRegex(ValueError,'membership'):self.verify()
        self.raw['commands']=copy.deepcopy(self.commands);self.raw['commands'][0]['argv']=['cargo','test']
        with self.assertRaisesRegex(ValueError,'mismatch'):self.verify()

    def test_no_tests_or_drifted_log_cannot_pass(self):
        self.raw['commands'][0]['tests']['passed']=0
        with self.assertRaisesRegex(ValueError,'not actually executed'):self.verify()
        self.raw['commands'][0]['tests']['passed']=72;(self.root/'0.log').write_text('changed')
        with self.assertRaisesRegex(ValueError,'log drift'):self.verify()

    def test_runner_source_and_dependency_fingerprint_must_match(self):
        self.raw['inputs_after']={**self.inputs,'source':'changed'}
        with self.assertRaisesRegex(ValueError,'drift'):self.verify()


if __name__=='__main__':unittest.main()
