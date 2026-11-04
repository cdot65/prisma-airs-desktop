from pathlib import Path
import os,sys,json,time
root=Path.cwd()
sys.path.insert(0,str(root/"tools/native-acceptance"))
import acceptance_p11 as core
import fixture_app as fx
out=root/".evidence-private/geometry-probe-01/out"
out.mkdir()
os.environ["AIRS_NATIVE_PROCESS_RECORD"]=str(out/"candidate-processes.json")
prepared,bundle=core.verify_prepare(root/"docs/planning/evidence/P11/native-prepare.json")
log=[];app=fx.App.__new__(fx.App)
app.bundle=str(bundle);app.candidate=fx.candidate.bind(str(bundle));app.log=log
app.ax_bin="/var/folders/p7/f_99nxmj7s16qrxbl0cbpz4r0000gn/T/airs-p11-owned-j9e8xcq6/ax"
app.pid=None
statefile=Path(fx.DATA)/"state.json"
before=statefile.read_bytes()
assert not app.candidate.matching(app.candidate.app_path) and not app.candidate.matching(app.candidate.engine_path)
result={"schema":"airs-desktop/owned-geometry-probe/1","executable_sha256":prepared["executable_sha256"],"measurements":[],"state_before_sha256":core.sha(statefile)}
try:
 app.launch()
 for requested in ((1440,848,0,0),(1440,852),(1440,900),(1440,852)):
  app.front()
  app.ax("resize",*requested)
  time.sleep(1)
  tree=app.tree()
  rows=[n for n in tree if n.get("role") in ("AXWindow","AXWebArea","AXScrollArea")]
  result["measurements"].append({"requested":requested,"owned_frame":json.loads(app.ax("frame")),"nodes":rows})
finally:
 app.quit()
 result["state_after_sha256"]=core.sha(statefile)
 result["fixture_state_unchanged"]=statefile.read_bytes()==before
 result["actions"]=log
 (out/"result.json").write_text(json.dumps(result,indent=2)+"\n")
 assert result["fixture_state_unchanged"],"diagnostic launch changed fixture state; preserve and inspect"
