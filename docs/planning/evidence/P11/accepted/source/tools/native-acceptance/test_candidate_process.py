"""Synthetic process-table tests: never launch or signal an OS process."""
import json,plistlib,signal,sys,tempfile,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parent))
from candidate_process import Candidate,Identity,FIXTURE_ID,EXECUTABLE
class Processes:
    def __init__(self):self.rows={};self.signals=[]
    def snapshot(self,pid):return self.rows.get(pid)
    def pids(self):return list(self.rows)
    def send(self,pid,sig):self.signals.append((pid,sig))
class Ownership(unittest.TestCase):
    def setUp(self):
        self.temp=tempfile.TemporaryDirectory();self.root=Path(self.temp.name)/'Prisma AIRS Desktop Fixture.app'
        p=self.root/'Contents/MacOS';p.mkdir(parents=True);(p/EXECUTABLE).write_text('synthetic executable')
        (self.root/'Contents/Info.plist').write_bytes(plistlib.dumps({'CFBundleIdentifier':FIXTURE_ID,'CFBundleExecutable':EXECUTABLE}))
        self.processes=Processes();self.candidate=Candidate(self.root,self.processes)
    def tearDown(self):self.temp.cleanup()
    def owned(self):
        row=Identity(10,self.candidate.app_path,(123,456),1);self.processes.rows[10]=row;self.candidate.adopt_child(10,1);return row
    def test_owned_exact_child_can_be_signaled(self):
        self.owned();self.candidate.signal_app();self.assertEqual(self.processes.signals,[(10,signal.SIGTERM)])
    def test_foreign_same_basename_old_namespace_and_regex_lookalike_are_not_signaled(self):
        for path in ['/foreign/Prisma AIRS Desktop Fixture.app/Contents/MacOS/'+EXECUTABLE,'/old/Prisma AIRS Desktop v2 Fixture.app/Contents/MacOS/prisma-airs-desktop-v2',self.candidate.app_path.replace('Fixture.app','FixtureXapp')]:
            self.processes.rows={11:Identity(11,path,(1,1),1)};self.candidate.signal_app()
        self.assertEqual(self.processes.signals,[])
    def test_unowned_same_candidate_is_not_adopted(self):
        self.processes.rows[10]=Identity(10,self.candidate.app_path,(1,1),99)
        with self.assertRaises(RuntimeError):self.candidate.app_pid()
        self.assertEqual(self.processes.signals,[])
    def test_reused_pid_cannot_be_signaled_even_with_same_path(self):
        old=self.owned();self.processes.rows[10]=Identity(10,old.executable,(124,1),old.parent)
        self.assertFalse(self.candidate.signal_identity(old,signal.SIGKILL));self.assertEqual(self.processes.signals,[])
    def test_duplicate_candidate_processes_refuse_ui_and_cleanup(self):
        self.owned();self.processes.rows[11]=Identity(11,self.candidate.app_path,(1,1),1)
        with self.assertRaises(RuntimeError):self.candidate.app_pid()
        self.assertEqual(self.processes.signals,[])
    def test_engine_must_be_exact_owned_descendant(self):
        self.owned();self.processes.rows.update({20:Identity(20,self.candidate.engine_path,(2,2),10),21:Identity(21,'/foreign/airs-harness',(2,3),10),22:Identity(22,self.candidate.engine_path,(2,4),99)})
        self.assertEqual(self.candidate.engines(),[20]);self.candidate.stop_engines();self.assertEqual(self.processes.signals,[(20,signal.SIGKILL)])
    def test_observed_engine_cleanup_survives_parent_exit_but_not_pid_reuse(self):
        self.owned();engine=Identity(20,self.candidate.engine_path,(2,2),10);self.processes.rows[20]=engine;self.candidate.engines();del self.processes.rows[10];self.processes.rows[20]=Identity(20,engine.executable,engine.birth,1)
        self.assertTrue(self.candidate.signal_engine(20));self.processes.rows[20]=Identity(20,engine.executable,(3,3),10)
        self.assertFalse(self.candidate.signal_engine(20));self.assertEqual(self.processes.signals,[(20,signal.SIGKILL)])
    def test_symlinked_engine_cannot_escape_the_bundle(self):
        external=Path(self.temp.name)/'foreign-engine';external.write_text('foreign')
        engine=self.root/'Contents/Resources/harness/airs-harness';engine.parent.mkdir(parents=True);engine.symlink_to(external)
        with self.assertRaises(ValueError):Candidate(self.root,self.processes)
        self.assertEqual(self.processes.signals,[])
    def test_production_or_missing_bundle_is_rejected(self):
        p=self.root/'Contents/Info.plist';p.write_bytes(plistlib.dumps({'CFBundleIdentifier':'io.cdot.airs-desktop','CFBundleExecutable':EXECUTABLE}))
        with self.assertRaises(ValueError):Candidate(self.root,self.processes)
        with self.assertRaises(FileNotFoundError):Candidate(self.root/'missing',self.processes)
if __name__=='__main__':unittest.main()
