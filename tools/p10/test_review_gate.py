from pathlib import Path
import tempfile
import unittest
from unittest.mock import patch
import review_gate


class ReleaseProofTests(unittest.TestCase):
    def test_isolated_release_interpreter_can_load_qualification_siblings(self):
        import subprocess
        root=Path(__file__).resolve().parents[2]
        result=subprocess.run(["/usr/bin/python3","-I","-c","import runpy;runpy.run_path('tools/p10/review_gate.py');from packet import qualification;assert callable(qualification)"],cwd=root,capture_output=True,text=True)
        self.assertEqual(result.returncode,0,result.stderr)

    def test_missing_actual_p10_proof_blocks_release(self):
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory)
            with patch.object(review_gate,"actual_review",return_value={"subject":"P09"}):
                with self.assertRaises(FileNotFoundError):review_gate.release_prerequisites(root)

    def test_unbound_member_or_qualification_cannot_complete_release(self):
        import json
        with tempfile.TemporaryDirectory() as directory:
            root=Path(directory);path=root/"docs/planning/evidence/P10";path.mkdir(parents=True)
            proof=dict(schema="airs-desktop/p10-completion-proof/1",p09={"subject":"P09"},p10={"subject":"P10"},attempt="attempt-01",source_archive="docs/planning/evidence/P10/source-archive")
            (path/"completion-proof.json").write_text(json.dumps(proof))
            with patch.object(review_gate,"actual_review",side_effect=[proof["p09"],proof["p10"]]):
                with self.assertRaises(KeyError):review_gate.release_prerequisites(root)

    def test_actual_historical_transport_and_reject_output_mutations(self):
        import json,shutil
        root=Path(__file__).resolve().parents[2]
        original=root/"docs/planning/reviews/P09/attempt-02"
        request=json.loads((original/"request.json").read_text())
        request["execution_root"]=json.loads((root/"docs/planning/evidence/C45/activation-20261102-g11/p09-proof.json").read_text())["source_checkout"]
        request["schema_archive"]="docs/planning/evidence/C45/activation-20261102-g11/p09-source/docs/architecture/review-verdict.schema.json"
        self.assertTrue(review_gate.transport(root,original,request)["reviewer_sessions_observed"])
        for flag,wrong in [("-o","/wrong/verdict.json"),("--output-schema","/wrong/schema.json"),("-s","workspace-write")]:
            with self.subTest(flag=flag),tempfile.TemporaryDirectory() as directory:
                out=Path(directory)
                for name in ["invocation.json","request.json","feature-probe.log","thread-reply.log","verdict-returned.json"]:shutil.copy2(original/name,out/name)
                invocation=json.loads((out/"invocation.json").read_text());args=invocation["command_without_prompt"];args[args.index(flag)+1]=wrong
                (out/"invocation.json").write_text(json.dumps(invocation))
                with self.assertRaisesRegex(ValueError,"transport"):review_gate.transport(root,out,request)


if __name__ == "__main__":unittest.main()
