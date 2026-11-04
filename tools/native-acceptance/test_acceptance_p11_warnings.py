"""Pure saved-AX tests: no fixture, endpoint, app or UI starts."""
import copy
import json
from pathlib import Path
import sys
import types
import unittest
from unittest.mock import patch

import acceptance_p11_warnings as warnings

RAW = Path('docs/planning/evidence/P11/native-attempt-10-vm/failure-tree.json')

class App:
    def __init__(self, tree): self.value = tree
    def tree(self): return self.value

class Context:
    def __init__(self, tree):
        self.app = App(tree)
        self.calls = []
        self.log = []
        self.press = self.base
    @staticmethod
    def require(ok, message):
        if not ok: raise RuntimeError(message)
    def base(self, role, label, timeout):
        self.calls.append((role, label, timeout))
        return 'base-result'

class WarningActionAdmission(unittest.TestCase):
    @classmethod
    def setUpClass(cls): cls.raw = json.loads(RAW.read_text())
    def test_actual10_old_exact_label_is_absent_new_exact_label_admitted(self):
        self.assertFalse(any(n.get('role') == 'AXButton' and (n.get('label') or '').startswith('Open API keys') for n in self.raw))
        target = [n for n in self.raw if n.get('role') == 'AXButton' and n.get('label') == 'Manage API keys']
        self.assertEqual(len(target), 1)
        self.assertIs(target[0]['enabled'], True)
        ctx = Context(copy.deepcopy(self.raw))
        self.assertEqual(warnings.press(ctx, ctx.base, 'AXButton', 'Open API keys', 7), 'base-result')
        self.assertEqual(ctx.calls, [('AXButton', 'Manage API keys', 7)])
    def test_original_button_keeps_original_semantics(self):
        for enabled in (True, False):
            ctx = Context([{'role': 'AXButton', 'label': 'Open API keys', 'enabled': enabled}, *self.raw])
            warnings.press(ctx, ctx.base, 'AXButton', 'Open API keys')
            self.assertEqual(ctx.calls, [('AXButton', 'Open API keys', 12)])
    def test_all_other_labels_and_roles_delegate_without_tree_dependency(self):
        for role, label in [('AXButton', 'Recheck setup'), ('AXTextField', 'Open API keys'), ('AXButton', 'Open API keys extra')]:
            ctx = Context([])
            warnings.press(ctx, ctx.base, role, label, 3)
            self.assertEqual(ctx.calls, [(role, label, 3)])
    def test_missing_wrong_role_wrong_name_disabled_and_ambiguous_rejected(self):
        target = {'role': 'AXButton', 'label': 'Manage API keys', 'enabled': True}
        for tree in [[], [dict(target, role='AXPopUpButton')], [dict(target, label='Manage API key')],
                     [dict(target, label='Manage API keys extra')], [dict(target, enabled=False)],
                     [dict(target, enabled=None)], [target, target], [target, dict(target, label='Manage API keys extra')]]:
            with self.subTest(tree=tree):
                ctx = Context(tree)
                with self.assertRaises(RuntimeError): warnings.press(ctx, ctx.base, 'AXButton', 'Open API keys')
                self.assertEqual(ctx.calls, [])
    def test_scoped_run_preserves_old_flow_and_restores_on_success_or_failure(self):
        for fail in (False, True):
            ctx = Context(self.raw)
            original = ctx.press
            def flow(actual):
                self.assertIs(actual, ctx)
                actual.press('AXButton', 'Open API keys', 9)
                if fail: raise ValueError('owned-flow-failure')
                return 'flow-result'
            with patch.dict(sys.modules, {'acceptance_p11_setup': types.SimpleNamespace(warning_flows=flow)}):
                if fail:
                    with self.assertRaisesRegex(ValueError, 'owned-flow-failure'): warnings.run(ctx)
                else: self.assertEqual(warnings.run(ctx), 'flow-result')
            self.assertIs(ctx.press, original)
            self.assertEqual(ctx.calls, [('AXButton', 'Manage API keys', 9)])

if __name__ == '__main__': unittest.main()
