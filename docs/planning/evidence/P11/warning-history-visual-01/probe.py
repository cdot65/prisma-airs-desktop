from pathlib import Path
import os,sys,json,time
root=Path.cwd()
sys.path.insert(0,str(root/"tools/native-acceptance"))
import acceptance_p11 as core
import acceptance_p11_warning_text as text
import acceptance_c29 as c29
import fixture_app as fx
out=root/"docs/planning/evidence/P11/warning-history-visual-01"
out.mkdir()
os.environ["AIRS_NATIVE_PROCESS_RECORD"]=str(out/"candidate-processes.json")
prepared,bundle=core.verify_prepare(root/"docs/planning/evidence/P11/native-prepare.json")
app=fx.App.__new__(fx.App)
app.bundle=str(bundle);app.candidate=fx.candidate.bind(str(bundle));app.log=[]
app.ax_bin="/var/folders/p7/f_99nxmj7s16qrxbl0cbpz4r0000gn/T/airs-p11-owned-j9e8xcq6/ax"
app.pid=None
statefile=Path(fx.DATA)/"state.json"
before=statefile.read_bytes()
prior=root/"docs/planning/evidence/P11/native-attempt-12-vm/observations.json"
assert json.loads(before)==json.loads((prior.parent/"fixture-state-final.json").read_text())
assert not app.candidate.matching(app.candidate.app_path) and not app.candidate.matching(app.candidate.engine_path)
receipt={"schema":"airs-desktop/visual-supplement/1","purpose":"Show resolved Supply Chain setup history already measured by native attempt12; no assertion replay","prior_observations":{"path":str(prior.relative_to(root)),"sha256":core.sha(prior)},"executable_sha256":prepared["executable_sha256"],"bundle_sha256":prepared["bundle_sha256"],"source_sha256":core.sha(__file__),"state_before_sha256":core.sha(statefile)}
exit_code=1
try:
 app.launch()
 app.ax("press","AXButton","Settings")
 tree,ready=app.wait_for(lambda t:app.node(t,"AXHeading","Settings") is not None)
 assert ready,"Settings did not open"
 app.ax("focus","AXButton","Export diagnostics")
 for attempt in range(3):
  app.keycode(121)
  tree=app.tree()
  frame=json.loads(app.ax("frame"))
  visible=[n for n in tree if n.get("role")=="AXStaticText" and n.get("label")=="Passed" and c29.inside(frame,n)]
  if len(visible)==1 and text.has_text(app.has_text,tree,"Latest check: Passed"):break
 else:raise RuntimeError("Resolved setup status was not visible in owned viewport")
 c29.NATIVE=str(out)
 image=c29.capture(app,"resolved-warning-history")
 assert image,"visual supplement capture missing"
 (out/"history-visible-tree.json").write_text(json.dumps(tree,indent=2)+"\n")
 receipt.update(capture={"path":image,"sha256":core.sha(root/image)},visible_passed_node=visible[0],owned_window_frame=frame,credential_input=False,assertions_executed=[],original_capture_note="Attempt12 image shows the history container; this supplement shows its already-passing resolved record.")
 exit_code=0
finally:
 app.quit()
 receipt.update(exit_code=exit_code,state_after_sha256=core.sha(statefile),fixture_state_unchanged=statefile.read_bytes()==before,actions=app.log)
 (out/"receipt.json").write_text(json.dumps(receipt,indent=2)+"\n")
 assert receipt["fixture_state_unchanged"],"Visual launch changed persisted fixture state"
raise SystemExit(exit_code)
