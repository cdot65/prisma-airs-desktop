from pathlib import Path
import tempfile
import unittest
from qualification import sha
from stages import verify_completed,verify_conditions


class StageBindingTests(unittest.TestCase):
    def test_original_logs_raw_records_and_evidence_are_immutable(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);folder=root/"stage";folder.mkdir()
            for name in ["stdout.log","stderr.log","raw.json","capture.json"]:(folder/name).write_text("actual synthetic stage output")
            state=dict(exit_code=0,argv=["driver"],source_fingerprint="source",candidate_sha256="candidate",stdout_sha256=sha(folder/"stdout.log"),stderr_sha256=sha(folder/"stderr.log"),artifacts={"stage/raw.json":sha(folder/"raw.json"),"stage/capture.json":sha(folder/"capture.json")})
            self.assertTrue(verify_completed(root,folder,state,["driver"],"source","candidate"))
            for name in ["stdout.log","stderr.log","raw.json","capture.json"]:
                with self.subTest(name=name):
                    path=folder/name;before=path.read_text();path.write_text("edited")
                    with self.assertRaisesRegex(ValueError,"drift"):verify_completed(root,folder,state,["driver"],"source","candidate")
                    path.write_text(before)
            with self.assertRaisesRegex(ValueError,"command/source"):verify_completed(root,folder,state,["other-driver"],"source","candidate")

    def test_superseded_raw_failure_missing_evidence_is_retained_as_missing(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);folder=root/"stage";folder.mkdir()
            for name in ["stdout.log","stderr.log"]:(folder/name).write_text("actual synthetic output")
            state=dict(exit_code=0,argv=["driver"],source_fingerprint="s",stdout_sha256=sha(folder/"stdout.log"),stderr_sha256=sha(folder/"stderr.log"),artifacts={"superseded-missing.json":None})
            self.assertTrue(verify_completed(root,folder,state,["driver"],"s"))
            (root/"superseded-missing.json").write_text("invented evidence")
            with self.assertRaisesRegex(ValueError,"missing superseded"):verify_completed(root,folder,state,["driver"],"s")

    def test_recovery_checks_toolchain_dependencies_flags_and_machine(self):
        current=dict(source="s",dependencies="d",build_flags="b")
        candidate=dict(runner_inputs=current,conditions=dict(platform="VM-macOS",hostname="worf-vm"))
        self.assertTrue(verify_conditions(candidate,current,"VM-macOS","worf-vm"))
        for field in current:
            changed=dict(current);changed[field]="changed"
            with self.assertRaisesRegex(ValueError,"identity changed"):verify_conditions(candidate,changed,"VM-macOS","worf-vm")
        for platform,host in [("different","worf-vm"),("VM-macOS","another-host")]:
            with self.assertRaisesRegex(ValueError,"conditions changed"):verify_conditions(candidate,current,platform,host)


if __name__=='__main__':unittest.main()
