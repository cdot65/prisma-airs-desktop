"""Pure saved-AX admission tests; no app or endpoint starts."""
import copy
import json
from pathlib import Path
import unittest

import acceptance_p11_roles as roles

RAW = Path('docs/planning/evidence/P11/native-attempt-07-vm/failure-tree.json')
class App:
    def __init__(self, tree): self.value = tree
    def tree(self): return self.value
class Context:
    def __init__(self, tree): self.app = App(tree); self.calls = []
    @staticmethod
    def require(ok, message):
        if not ok: raise RuntimeError(message)
    def base(self, role, label, timeout): self.calls.append((role, label, timeout)); return 'base-result'

class RoleAdmission(unittest.TestCase):
    @classmethod
    def setUpClass(cls): cls.raw = json.loads(RAW.read_text())
    def test_actual07_popup_is_admitted_without_a_button(self):
        self.assertFalse(any(n.get('role')=='AXButton' and n.get('label')=='Contents' for n in self.raw))
        ctx=Context(self.raw)
        self.assertEqual(roles.press(ctx,ctx.base,'AXButton','Contents'),'base-result')
        self.assertEqual(ctx.calls,[('AXPopUpButton','Contents',12)])
    def test_normal_button_keeps_original_path(self):
        ctx=Context([{'role':'AXButton','label':'Contents explanation','enabled':False}, *self.raw])
        roles.press(ctx,ctx.base,'AXButton','Contents',7)
        self.assertEqual(ctx.calls,[('AXButton','Contents',7)])
    def test_unrelated_controls_delegate(self):
        ctx=Context([]);roles.press(ctx,ctx.base,'AXButton','Close',3)
        self.assertEqual(ctx.calls,[('AXButton','Close',3)])
    def test_wrong_name_role_disabled_and_ambiguous_fail_closed(self):
        candidate={'role':'AXPopUpButton','label':'Contents','enabled':True}
        for tree in [[],[dict(candidate,label='Contents extra')],[dict(candidate,role='AXMenuButton')],[dict(candidate,enabled=False)],[dict(candidate,enabled=None)],[candidate,candidate],[dict(candidate,label='Contents extra'),candidate]]:
            with self.subTest(tree=tree):
                ctx=Context(tree)
                with self.assertRaises(RuntimeError):roles.press(ctx,ctx.base,'AXButton','Contents')
                self.assertEqual(ctx.calls,[])

if __name__=='__main__':unittest.main()
