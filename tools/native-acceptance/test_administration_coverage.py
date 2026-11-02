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
                 node("AXHeading", "gateway-key-1", 4), node("AXButton", "Edit gateway-key-1", 4),
                 node("AXButton", "Rotate gateway-key-1", 4), node("AXButton", "Remove gateway-key-1", 4)]
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
        self.assertEqual(state["detail_actions"], ["Edit gateway-key-1", "Rotate gateway-key-1", "Remove gateway-key-1", "Close details"])

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



class GatewayWorkspacePrerequisite(unittest.TestCase):
    def test_key_inspection_selects_the_required_workspace_before_key_controls(self):
        from unittest.mock import patch
        import acceptance_c31 as driver
        class App:
            def __init__(self):
                self.workspace = False
                self.kind = 'Service keys'
                self.detail = False
                self.actions = []
            def tree(self):
                if not self.workspace:
                    return product('AI Gateway', [node('AXRadioButton', 'Keys', 3),
                        node('AXRadioButton', 'Team A · us', 3),
                        node('AXStaticText', 'Choose a workspace to read its configuration.', 3)])
                tree = gateway_tree(detail=self.detail, kind=self.kind)
                if not self.detail:
                    tree.append(node('AXButton', 'Details for gateway-key-1', 3))
                return tree
            def wait_for(self, predicate, timeout=12):
                tree = self.tree()
                return tree, bool(predicate(tree))
            def ax(self, action, role, label, occurrence=0):
                self.actions.append(label)
                if label.startswith('Team A'):
                    self.workspace = True
                elif label in ['Service keys', 'User keys']:
                    self.kind = label
                elif label.startswith('Details for '):
                    self.detail = True
                elif label == 'Close details':
                    self.detail = False
        app = App()
        with patch.object(driver.c29, 'save_tree', return_value='synthetic-tree.json'), patch.object(driver.c29, 'capture', return_value='synthetic-capture.png'):
            result = driver.gateway_key_checks(app)
        self.assertTrue(coverage.gateway_keys_complete(result))
        self.assertEqual(app.actions[0], 'Team A · us')
        self.assertEqual(set(result['kinds']), {'Service keys', 'User keys'})


class SuccessfulGatewayDetails(unittest.TestCase):
    def test_error_loading_absent_and_incomplete_details_cannot_prove_restrictions(self):
        for message in ['The service did not find this resource.', 'Reading details…',
                        'The workspace’s complete inventory does not contain this item, so it is not shown.',
                        'This item was not in the part of the workspace’s inventory that could be read. It is not shown, and this does not mean it is absent.']:
            with self.subTest(message=message):
                tree = gateway_tree()
                tree.insert(-1, node('AXStaticText', message, 4))
                self.assertFalse(coverage.gateway_key_state(tree, 'Service keys', require_detail=True)['ok'])

    def test_every_listed_key_has_a_successful_scoped_detail_fixture(self):
        import threading
        from types import SimpleNamespace
        import mgmt_server
        handler = object.__new__(mgmt_server.Handler)
        handler.server = SimpleNamespace(lock=threading.Lock(), gw={'workspaces': [], 'archived': set(),
            'deleted': set(), 'created': {'api-keys/service': []}, 'renamed': {}, 'scopes': {}})
        for resource in ['service', 'user']:
            status, listing = handler.gateway('/ai_gw/v2/api-keys/' + resource,
                                              {'workspace_id': mgmt_server.GW_A}, {})
            self.assertEqual(status, 200)
            self.assertTrue(listing['data'])
            for item in listing['data']:
                status, detail = handler.gateway('/ai_gw/v2/api-keys/' + resource + '/' + item['id'],
                                                {'workspace_id': mgmt_server.GW_A}, {})
                self.assertEqual(status, 200, (resource, item['id']))
                self.assertEqual(detail['id'], item['id'])
                self.assertEqual(detail['name'], item['name'])
                self.assertEqual(detail['workspace_id'], mgmt_server.GW_A)
                self.assertTrue({'id', 'name', 'object', 'status', 'key', 'scopes', 'workspace_id'} <= set(detail))

if __name__ == "__main__":
    unittest.main()
