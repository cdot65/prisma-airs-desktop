import copy
import json
from pathlib import Path
import tempfile
import unittest
from qualification import verify, sha


class CoverageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.candidate = dict(executable_sha256="exe", bundle_sha256="bundle", helper_closure={"driver": "hash"}, build_flags=["fixture"], conditions={"os": "VM"}, source_inventory_sha256="current", execution_root="/synthetic", runner_inputs={"source":"current"})
        self.ref = self.write("candidate.json", self.candidate)
        self.log = self.write("log.txt", "actual synthetic output")
        self.result = dict(status="pass", exit_code=0, execution_id="run-1", argv=["synthetic-check"], cwd="/synthetic", started_utc="start", finished_utc="end", required_ids=["N01"], log=self.log, candidate=self.ref)
        raw=dict(schema="airs-v2/run/1",status="passed",failure=None,inputs_before={"source":"current"},inputs_after={"source":"current"},commands=[dict(label="check",argv=["synthetic-check"],cwd="/synthetic",status="passed",exit_code=0,note=None,output_sha256=self.log["sha256"],log="log.txt",tests=dict(passed=1,failed=0))])
        self.result["raw"]=self.write("raw.json",raw)
        self.result_ref = self.write("result.json", self.result)
        self.plan = dict(schema="airs-desktop/required-coverage/1", requirements=[dict(id="N01",kind="command",label="check",argv=["synthetic-check"],expects_tests=True, affected=True, must_execute=False, dependencies=["native"] )])
        self.record = dict(schema="airs-desktop/qualified-coverage/1", source_inventory_sha256="current", candidate=self.ref, plan=self.write("plan.json", self.plan), coverage=[dict(id="N01", disposition="executed", result=self.result_ref)])

    def write(self, name, value):
        path = self.root/name
        path.write_text(json.dumps(value) if not isinstance(value, str) else value)
        return dict(path=name, sha256=sha(path))

    def test_current_execution_and_phase_exact_reuse(self):
        self.assertTrue(verify(self.root, self.record)["qualified"])
        # A phase consumes the actual chunk execution without executing it again.
        self.record["coverage"][0]["disposition"] = "reused_exact"
        self.assertEqual(verify(self.root, self.record)["dispositions"], {"reused_exact": 1})

    def test_test_only_source_change_allows_exact_artifact_reuse(self):
        new = dict(self.candidate, source_inventory_sha256="test-only-new")
        self.record.update(candidate=self.write("new-candidate.json", new), source_inventory_sha256="test-only-new")
        self.record["coverage"][0]["disposition"] = "reused_exact"
        self.assertTrue(verify(self.root, self.record)["qualified"])
        self.record["coverage"][0]["disposition"] = "executed"
        with self.assertRaisesRegex(ValueError, "historical"):
            verify(self.root, self.record)

    def test_every_exact_boundary_mismatch_rejected(self):
        for field in ["executable_sha256", "bundle_sha256", "helper_closure", "build_flags", "conditions"]:
            with self.subTest(field=field):
                record=copy.deepcopy(self.record)
                candidate=dict(self.candidate); candidate[field]="different"
                record["candidate"]=self.write("different.json", candidate)
                record["coverage"][0]["disposition"]="reused_exact"
                with self.assertRaisesRegex(ValueError, "differ"):verify(self.root,record)

    def test_forged_coverage_claim_cannot_replace_raw_failure(self):
        raw=json.loads((self.root/"raw.json").read_text());raw["commands"][0]["exit_code"]=1
        result=dict(self.result,raw=self.write("raw-fail.json",raw))
        self.record["coverage"][0]["result"]=self.write("forged-pass.json",result)
        with self.assertRaisesRegex(ValueError,"raw runner"):verify(self.root,self.record)

    def test_native_claim_requires_actual_passing_bound_assertion(self):
        self.plan["requirements"][0].update(kind="native",argv=["native-driver"])
        self.record["plan"]=self.write("plan.json",self.plan)
        evidence=self.write("capture.json",{"observed":"native synthetic fixture"})
        raw=dict(schema="airs-v2/native-acceptance/1",app={"executable_sha256":"exe"},checks=[dict(id="N01",result="pass",observation="observed",evidence=[evidence["path"]])])
        execution=dict(self.result,argv=["native-driver"],raw=self.write("native.json",raw),evidence={evidence["path"]:evidence["sha256"]})
        self.record["coverage"][0]["result"]=self.write("native-execution.json",execution)
        self.assertTrue(verify(self.root,self.record)["qualified"])
        raw["checks"][0]["result"]="fail";execution["raw"]=self.write("native-failed.json",raw)
        self.record["coverage"][0]["result"]=self.write("native-execution-false.json",execution)
        with self.assertRaisesRegex(ValueError,"raw native assertion"):verify(self.root,self.record)

    def test_unapproved_command_rejected(self):
        result=dict(self.result,argv=["different"])
        self.record["coverage"][0]["result"]=self.write("other.json",result)
        with self.assertRaisesRegex(ValueError,"approved plan"):verify(self.root,self.record)

    def test_missing_duplicate_unknown_ids_rejected(self):
        for rows in [[], self.record["coverage"]*2, [dict(id="unknown", disposition="executed")]]:
            with self.subTest(rows=rows):
                with self.assertRaisesRegex(ValueError,"coverage"):verify(self.root,dict(self.record,coverage=rows))

    def test_failed_and_edited_execution_rejected(self):
        result=dict(self.result,status="fail",exit_code=1)
        self.record["coverage"][0]["result"]=self.write("failed.json",result)
        with self.assertRaisesRegex(ValueError,"passing"):verify(self.root,self.record)
        self.record["coverage"][0]["result"]=self.result_ref
        (self.root/"log.txt").write_text("edited")
        with self.assertRaisesRegex(ValueError,"hash drift"):verify(self.root,self.record)

    def test_unsupported_retirement_rejected(self):
        self.record["coverage"]=[dict(id="N01",disposition="not_applicable",reason="convenient")]
        with self.assertRaisesRegex(ValueError,"retirement"):verify(self.root,self.record)

    def test_changed_test_or_lint_target_must_execute(self):
        self.plan["requirements"][0]["must_execute"]=True
        self.record["plan"]=self.write("plan.json",self.plan)
        self.record["coverage"][0]["disposition"]="reused_exact"
        with self.assertRaisesRegex(ValueError,"must execute"):verify(self.root,self.record)

    def test_unrelated_helper_edit_does_not_invalidate_driver(self):
        self.plan["requirements"][0]["helper_paths"]=["driver"]
        self.record["plan"]=self.write("plan.json",self.plan)
        current=dict(self.candidate,helper_closure={"driver":"hash","unrelated":"new"})
        self.record["candidate"]=self.write("current.json",current)
        self.record["coverage"][0]["disposition"]="reused_exact"
        self.assertTrue(verify(self.root,self.record)["qualified"])

    def test_affected_target_cannot_be_carried(self):
        self.record["coverage"][0]["disposition"]="carried_unaffected"
        with self.assertRaisesRegex(ValueError,"affected"):verify(self.root,self.record)

    def test_unaffected_historical_result_preserves_original_binding(self):
        self.plan["requirements"][0]["affected"]=False
        dependency=self.write("native", "unchanged native dependency")
        self.record["plan"]=self.write("plan.json",self.plan)
        self.record["coverage"][0].update(disposition="carried_unaffected",impact=dict(reason="exact native dependency closure unchanged",dependencies=["native"],original_candidate_sha256=self.ref["sha256"],proof=self.write("impact.json",dict(unchanged_dependencies=["native"],current_source_inventory_sha256="current",original_candidate_sha256=self.ref["sha256"],file_bindings={"native":dict(original_sha256=dependency["sha256"],current_sha256=dependency["sha256"],original_file=dependency)}))))
        self.assertTrue(verify(self.root,self.record)["qualified"])
        self.record["coverage"][0]["impact"]["dependencies"]=["unrelated"]
        with self.assertRaisesRegex(ValueError,"impact"):verify(self.root,self.record)


if __name__ == "__main__":unittest.main()
