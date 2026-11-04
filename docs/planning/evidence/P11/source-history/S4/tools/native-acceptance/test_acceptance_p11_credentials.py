"""Pure replay of the observed context-remount/storage-mode boundary."""
import copy
import json
from pathlib import Path
import unittest
from unittest.mock import patch
import acceptance_p11_credentials as adapter

RAW = Path('docs/planning/evidence/P11/native-attempt-08-vm/c49-session-key-1.json')
class App:
    def __init__(self): self.original=json.loads(RAW.read_text()); self.value=copy.deepcopy(self.original); self.writes=[]
    def tree(self): return self.value
    @staticmethod
    def node(tree,role,label,nth=0):
        rows=[n for n in tree if n.get('role')==role and (n.get('label') or '').startswith(label)]
        return rows[nth] if nth<len(rows) else None
    def ax(self,command,role,label,nth=0):
        assert command=='press' and role=='AXRadioButton' and label=='For this session only' and nth==0
        self.node(self.value,role,label,nth)['value']=1
        self.node(self.value,role,'In secure storage',nth)['value']=0
    def wait_for(self,predicate,timeout=12): return self.value,bool(predicate(self.value))
    def type_into(self,role,label,text,*args,**kwargs):
        session=self.node(self.value,'AXRadioButton','For this session only',0)['value']==1
        self.writes.append((role,label,session))
    def remount(self): self.value=copy.deepcopy(self.original)
class Context:
    def __init__(self): self.app=App();self.log=[]
    @staticmethod
    def require(ok,message):
        if not ok:raise RuntimeError(message)

def two_submissions(ctx,bootstrap):
    # Same call pattern as the immutable helper: select once before the loop,
    # while the actual saved AX tree proves each context remount resets it.
    adapter.settings._radio(ctx,'For this session only',0)
    for index in (1,2):
        ctx.app.type_into('AXTextField','AIRS API key',f'synthetic-c49-session-key-{index}',secure=True)
        if not ctx.app.writes[-1][2]:raise AssertionError('Scan key saved in secure storage; expected session-only')
        ctx.app.remount()
    return 'old-helper-result'

class CredentialAdmission(unittest.TestCase):
    def test_two_submissions_reselect_after_actual_remount(self):
        ctx=Context()
        self.assertEqual(ctx.app.node(ctx.app.tree(),'AXRadioButton','For this session only')['value'],0)
        original=ctx.app.type_into
        with patch.object(adapter.settings,'_tenant_controls',two_submissions):
            self.assertEqual(adapter.run(ctx,'P11 Northwind'),'old-helper-result')
        self.assertEqual([x[2] for x in ctx.app.writes],[True,True])
        self.assertEqual(ctx.app.type_into,original)
    def test_restores_type_hook_when_old_helper_fails(self):
        ctx=Context();original=ctx.app.type_into
        with patch.object(adapter.settings,'_tenant_controls',side_effect=RuntimeError('owned failure')):
            with self.assertRaisesRegex(RuntimeError,'owned failure'):adapter.run(ctx,'P11 Northwind')
        self.assertEqual(ctx.app.type_into,original)

    def test_unrelated_secret_entry_is_unchanged(self):
        ctx=Context()
        def helper(ctx,bootstrap):
            ctx.app.type_into('AXTextField','Client secret','synthetic-other-input',secure=True)
            return two_submissions(ctx,bootstrap)
        with patch.object(adapter.settings,'_tenant_controls',helper):adapter.run(ctx,'P11 Northwind')
        self.assertEqual(ctx.app.writes[0],('AXTextField','Client secret',False))
        self.assertEqual([x[2] for x in ctx.app.writes[1:]],[True,True])
    def test_wrong_synthetic_sequence_fails_before_entry(self):
        ctx=Context();original=ctx.app.type_into
        def helper(ctx,bootstrap):ctx.app.type_into('AXTextField','AIRS API key','synthetic-c49-session-key-2',secure=True)
        with patch.object(adapter.settings,'_tenant_controls',helper):
            with self.assertRaisesRegex(RuntimeError,'sequence'):adapter.run(ctx,'P11 Northwind')
        self.assertEqual(ctx.app.writes,[])
        self.assertEqual(ctx.app.type_into,original)

if __name__=='__main__':unittest.main()
