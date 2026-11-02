"""Focused AX scoping and enforced C31 requirement regressions; no UI calls."""
from pathlib import Path
import copy
import importlib.util
import sys
import unittest

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
sys.path.insert(0, str(HERE))
import administration_coverage as coverage


def node(role, label, depth, **extra):
    return dict(role=role, label=label, depth=depth, **extra)


def product(name, body):
    heading = name + " product description"
    return [node("AXGroup", name + " tabs Keys", 2),
            node("AXHeading", heading, 3), node("AXButton", heading, 4), *body]


def gateway_tree(forbidden=False, detail=True, kind="Service keys"):
    # A legitimate Runtime rotation and a similarly named navigation button
    # coexist with Gateway. They are not Gateway inventory authority.
    tree = [node("AXButton", "AI Gateway", 1)]
    tree += product("AI Runtime Security", [
        node("AXButton", "Issue additional key…", 3),
        node("AXButton", "Finish rotation…", 3)])
    body = [node("AXTabGroup", "AI Gateway tabs", 3), node("AXRadioButton", "Keys", 4),
            node("AXRadioButton", "Service keys", 3), node("AXRadioButton", "User keys", 3),
            node("AXTable", kind, 3)]
    if detail:
        body += [node("AXGroup", "gateway-key-1", 3, subrole="AXLandmarkRegion"),
                 node("AXHeading", "gateway-key-1", 4), node("AXButton", "Remove…", 4)]
        if forbidden:
            body.append(node("AXButton", "Issue additional key…", 4))
        body.append(node("AXButton", "Close details", 4))
    return tree + product("AI Gateway", body)


def complete_observations():
    result = {"kinds": {}}
    for kind in ("Service keys", "User keys"):
        list_state = coverage.gateway_key_state(gateway_tree(detail=False, kind=kind), kind)
        detail = coverage.gateway_key_state(gateway_tree(kind=kind), kind, require_detail=True)
        detail["row"] = "Details for gateway-key-1"
        result["kinds"][kind] = {"list": list_state, "row_labels": [detail["row"]], "details": [detail]}
    return result


class AdministrationScoping(unittest.TestCase):
    def test_runtime_rotation_does_not_taint_gateway(self):
        state = coverage.gateway_key_state(gateway_tree(), "Service keys", require_detail=True)
        self.assertTrue(state["ok"])
        self.assertEqual(state["forbidden_actions"], [])
        self.assertEqual(state["detail_actions"], ["Remove…", "Close details"])

    def test_gateway_rotation_action_is_rejected(self):
        state = coverage.gateway_key_state(gateway_tree(forbidden=True), "Service keys", require_detail=True)
        self.assertFalse(state["ok"])
        self.assertEqual(state["forbidden_actions"], ["Issue additional key…"])

    def test_missing_card_table_or_detail_is_not_negative_proof(self):
        for tree in [[], gateway_tree(detail=False),
                     [n for n in gateway_tree() if n["role"] != "AXTable"]]:
            with self.subTest(tree=tree):
                self.assertFalse(coverage.gateway_key_state(tree, "Service keys", require_detail=True)["ok"])

    def test_header_action_uses_board_card_not_navigation_rail(self):
        tree = gateway_tree()
        header = coverage.product_header(tree, "AI Gateway")
        self.assertEqual(header["label"], "AI Gateway product description")
        calls = []
        class App:
            def ax(self, *args):
                calls.append(args)
        coverage.act_on_node(App(), tree, header, "focus")
        self.assertEqual(calls, [("focus", "AXButton", "AI Gateway product description", 0)])

    def test_gate_only_product_rejects_other_interactive_actions(self):
        name = "Agentic Identity Security"
        gate = "needs an admitted " + name + " service"
        tree = product(name, [node("AXStaticText", gate, 3)])
        self.assertTrue(coverage.gated_product(tree, name, gate))
        tree.append(node("AXButton", "New identity…", 3))
        self.assertFalse(coverage.gated_product(tree, name, gate))

    def test_both_key_kinds_and_every_returned_detail_are_required(self):
        value = complete_observations()
        self.assertTrue(coverage.gateway_keys_complete(value))
        bad = copy.deepcopy(value)
        del bad["kinds"]["User keys"]
        self.assertFalse(coverage.gateway_keys_complete(bad))
        bad = copy.deepcopy(value)
        bad["kinds"]["Service keys"]["details"] = []
        self.assertFalse(coverage.gateway_keys_complete(bad))
        bad = copy.deepcopy(value)
        bad["kinds"]["User keys"]["row_labels"].append("Details for missing-key")
        self.assertFalse(coverage.gateway_keys_complete(bad))

    def test_missing_or_failed_assertion_fact_rejects_coverage(self):
        value = complete_observations()
        for kind in ["Service keys", "User keys"]:
            for location in ["list", "detail"]:
                original = value["kinds"][kind]["list"] if location == "list" else value["kinds"][kind]["details"][0]
                for field in original:
                    with self.subTest(kind=kind, location=location, missing=field):
                        bad = copy.deepcopy(value)
                        state = bad["kinds"][kind]["list"] if location == "list" else bad["kinds"][kind]["details"][0]
                        del state[field]
                        self.assertFalse(coverage.gateway_keys_complete(bad))
            bad = copy.deepcopy(value)
            bad["kinds"][kind]["details"][0]["forbidden_actions"] = ["Finish rotation…"]
            self.assertFalse(coverage.gateway_keys_complete(bad))


class RequiredNativeAssertions(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        path = ROOT / "docs/planning/evidence/P09/native-controller/native_requirements.py"
        spec = importlib.util.spec_from_file_location("c31_native_requirements", path)
        cls.requirements = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(cls.requirements)
        cls.required = cls.requirements.load_requirements(ROOT)

    def record(self, subject):
        return {"app": {"executable_sha256": "current-test-executable"},
                "checks": [{"id": cid, "result": "pass", "evidence": ["tools/native-acceptance/legacy_assertions.py"]}
                           for cid in sorted(self.required[subject])]}

    def test_all_original_c31_inventory_and_accessibility_checks_required(self):
        self.assertEqual(self.required["C31"], {"C31-N01", "C31-N02", "C31-N03", "C31-N04"})
        self.assertTrue({"C39-N01", "C39-N02", "C39-N03", "C39-N04"} <= self.required["C39"])

    def test_missing_failed_or_stale_required_assertion_rejected(self):
        for subject, ids in [("C31", ["C31-N01", "C31-N04"]), ("C39", ["C39-N03", "C39-N04"])]:
            baseline = self.record(subject)
            errors, _ = self.requirements.assess_record(baseline, ROOT, "current-test-executable", self.required[subject])
            self.assertEqual(errors, [])
            for cid in ids:
                for mutation in ["missing", "fail"]:
                    with self.subTest(cid=cid, mutation=mutation):
                        value = copy.deepcopy(baseline)
                        if mutation == "missing":
                            value["checks"] = [c for c in value["checks"] if c["id"] != cid]
                        else:
                            next(c for c in value["checks"] if c["id"] == cid)["result"] = "fail"
                        errors, _ = self.requirements.assess_record(value, ROOT, "current-test-executable", self.required[subject])
                        self.assertIn(cid + " " + mutation, errors)
            errors, _ = self.requirements.assess_record(baseline, ROOT, "different-executable", self.required[subject])
            self.assertIn("stale executable", errors)


if __name__ == "__main__":
    unittest.main()
