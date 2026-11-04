"""Pure sizing policy replay; no AX action or application starts."""
import copy
import json
from pathlib import Path
import unittest
from unittest.mock import patch
import acceptance_p11_geometry as geometry

RAW=Path('docs/planning/evidence/P11/native-attempt-09-vm/failure-tree.json')
class App:
    def __init__(self,ctx):self.ctx=ctx
    def tree(self):
        frame=self.ctx.value
        size=self.ctx.fixed_viewport or [frame[2],frame[3]-32]
        return [{'role':'AXWebArea','size':size}]
class Context:
    def __init__(self,fixed=None):
        tree=json.loads(RAW.read_text());self.value=next(n['pos']+n['size'] for n in tree if n.get('role')=='AXWindow')
        self.fixed_viewport=fixed;self.app=App(self);self.log=[];self.calls=[]
    def frame(self):return list(self.value)
    @staticmethod
    def require(ok,message):
        if not ok:raise RuntimeError(message)
    def resize(self,width,height,position=None):
        self.calls.append((width,height,position))
        if position is not None:raise AssertionError('repeated clamped position is forbidden')
        self.value[2:]=[width,height]

class GeometryPolicy(unittest.TestCase):
    def test_actual09_chrome_is32_and820_is_exact(self):
        ctx=Context()
        self.assertEqual(geometry._measure(ctx)[2],[0,32])
        with patch.object(geometry.time,'sleep'):frame=geometry.size_window(ctx,1440,820)
        self.assertEqual(ctx.calls,[(1440,852,None)])
        self.assertEqual(frame,[0,30,1440,852])
        self.assertEqual(ctx.log[-1]['actual_viewport'],[1440,820])
    def test_narrow600_viewport_is_not_outer600(self):
        ctx=Context()
        with patch.object(geometry.time,'sleep'):geometry.size_window(ctx,900,600)
        self.assertEqual(ctx.calls,[(900,632,None)])
        self.assertEqual(ctx.log[-1]['actual_viewport'],[900,600])
    def test_wrong_viewport_never_passes(self):
        ctx=Context(fixed=[1440,816])
        with patch.object(geometry.time,'sleep'):
            with self.assertRaisesRegex(RuntimeError,'required shell viewport'):geometry.size_window(ctx,1440,820)
        self.assertEqual(len(ctx.calls),3)
    def test_foreign_identity_error_is_not_retried(self):
        ctx=Context()
        with patch.object(ctx,'resize',side_effect=RuntimeError('fixture identity changed')) as resize:
            with self.assertRaisesRegex(RuntimeError,'identity changed'):geometry.size_window(ctx,1440,820)
        self.assertEqual(resize.call_count,1)
    def test_shell_hook_restored_on_failure(self):
        original=geometry.shell.size_window
        with patch.object(geometry.shell,'run',side_effect=RuntimeError('owned failure')):
            with self.assertRaisesRegex(RuntimeError,'owned failure'):geometry.run_shell(Context())
        self.assertIs(geometry.shell.size_window,original)

if __name__=='__main__':unittest.main()
