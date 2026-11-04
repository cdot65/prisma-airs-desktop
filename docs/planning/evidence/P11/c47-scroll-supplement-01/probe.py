from pathlib import Path
import os,sys,json,time
root=Path.cwd();sys.path.insert(0,str(root/"tools/native-acceptance"))
import acceptance_p11 as core, acceptance_c08 as c8, acceptance_c29 as c29, fixture_app as fx
out=root/".evidence-private/explain-scroll-01/out";out.mkdir()
os.environ["AIRS_NATIVE_PROCESS_RECORD"]=str(out/"candidate-processes.json")
prepared,bundle=core.verify_prepare(root/"docs/planning/evidence/P11/native-prepare.json")
work=Path("/var/folders/p7/f_99nxmj7s16qrxbl0cbpz4r0000gn/T/airs-p11-owned-j9e8xcq6")
app=fx.App.__new__(fx.App);app.bundle=str(bundle);app.candidate=fx.candidate.bind(str(bundle));app.log=[];app.ax_bin=str(work/"ax");app.pid=None
statefile=Path(fx.DATA)/"state.json";before=statefile.read_bytes()
assert not app.candidate.matching(app.candidate.app_path) and not app.candidate.matching(app.candidate.engine_path)
assert Path(c8.MGMT_ORIGIN_FILE).read_text().strip()=="https://localhost:62780"
mgmt=c8.MgmtEndpoint.__new__(c8.MgmtEndpoint);mgmt.certs=str(work/"scan");mgmt.dir=str(work/"mgmt");mgmt.port=62780;mgmt.proc=None
offset=len(mgmt.requests());mode_sha=core.sha(work/"mgmt/mode.json");roots_sha=core.sha(fx.ROOTS)
receipt={"schema":"airs-desktop/p11-drawer-scroll/1","started_utc":time.strftime("%Y-%m-%dT%H:%M:%SZ",time.gmtime()),"argv":[sys.executable,*sys.argv],"cwd":str(root),"operator":os.environ.get("AIRS_NATIVE_ORCHESTRATOR"),"executable_sha256":prepared["executable_sha256"],"bundle_sha256":prepared["bundle_sha256"],"source_sha256":core.sha(__file__),"helper_closure":core._binding.helper_closure(root,"acceptance_p11_warn"),"baseline_qualification":{"path":"docs/planning/evidence/P11/qualification-final-02/qualification.json","sha256":core.sha(root/"docs/planning/evidence/P11/qualification-final-02/qualification.json")},"state_before_sha256":core.sha(statefile),"facts":{}}
def frame():return json.loads(app.ax("frame"))
def headers(t):return [n for n in t if (n.get("label") or "").startswith("Request headers") and n.get("role") in ("AXDisclosureTriangle","AXButton","AXCheckBox")]
def anchor(t):
 return next(n for n in t if n.get("role")=="AXStaticText" and (n.get("label") or "").startswith("6 products"))
def save(name,obj):
 p=out/(name+".json");p.write_text(json.dumps(obj,indent=2)+"\n");return str(p.relative_to(root))
exit_code=1
try:
 mgmt.start();app.launch();app.ax("press","AXButton","Home")
 app.wait_for(lambda t:app.node(t,"AXHeading","Home") is not None)
 role=next(n["role"] for n in app.tree() if n.get("label")=="Show the calls")
 app.ax("press",role,"Show the calls")
 tree,ready=app.wait_for(lambda t:app.has_text(t,"The calls"))
 assert ready,"Explain did not open"
 for _ in range(3):
  n0=len(mgmt.requests());app.ax("press","AXButton","Refresh");time.sleep(.6)
  tree,ready=app.wait_for(lambda t:len(mgmt.requests())>n0 and app.has_text(t,"Read finished"),25)
  assert ready,"fixture refresh did not finish"
  tree=app.tree()
  if len(headers(tree))>=8:break
 tree=app.tree();controls=headers(tree);win=frame();main0=anchor(tree)
 assert len(controls)>=8,"fixture call history did not overflow"
 index=next(i for i,n in enumerate(controls) if n["pos"][1]>win[1]+win[3])
 first=controls[0];lower=controls[index]
 c29.NATIVE=str(out)
 before_image=c29.capture(app,"drawer-before-scroll")
 save("before-scroll",tree)
 app.ax("focus",first["role"],first["label"],0)
 for _ in range(6):
  app.keycode(121);tree=app.tree();current=headers(tree)
  assert len(current)==len(controls),"call list changed during scroll"
  if c29.inside(frame(),current[index]):break
 else:raise RuntimeError("lower call was not reached by drawer scrolling")
 main1=anchor(tree);first1=current[0];lower1=current[index]
 facts={"overflowing_fixture_history":len(controls)>=8,"lower_call_initially_below_viewport":lower["pos"][1]>win[1]+win[3],"lower_call_reached":c29.inside(frame(),lower1),"drawer_content_moved":first1["pos"][1]<first["pos"][1]-20,"underlying_page_position_unchanged":main0["pos"]==main1["pos"] and main0["size"]==main1["size"]}
 assert all(facts.values()),facts
 after_image=c29.capture(app,"drawer-after-scroll");save("after-scroll",tree)
 receipt.update(facts=facts,call_controls=len(controls),lower_call_index=index,before_anchor=main0,after_anchor=main1,before_first_call=first,after_first_call=first1,before_lower_call=lower,after_lower_call=lower1,captures=[{"path":p,"sha256":core.sha(root/p)} for p in (before_image,after_image)])
 app.ax("press","AXButton","Close Explain")
 _,closed=app.wait_for(lambda t:app.node(t,"AXButton","Close Explain") is None)
 assert closed,"Explain did not close"
 core.verify_prepare(root/"docs/planning/evidence/P11/native-prepare.json")
 exit_code=0
except BaseException as error:
 receipt["error"]=type(error).__name__+": "+str(error)
 if app.pid:
  try:save("failure-tree",app.tree())
  except Exception:pass
finally:
 try:app.quit()
 finally:mgmt.stop()
 receipt.update(exit_code=exit_code,finished_utc=time.strftime("%Y-%m-%dT%H:%M:%SZ",time.gmtime()),fixture_state_unchanged=statefile.read_bytes()==before,state_after_sha256=core.sha(statefile),fixture_modes_unchanged=core.sha(work/"mgmt/mode.json")==mode_sha,trust_roots_unchanged=core.sha(fx.ROOTS)==roots_sha,actions=app.log,new_request_count=len(mgmt.requests())-offset)
 (out/"receipt.json").write_text(json.dumps(receipt,indent=2)+"\n")
raise SystemExit(exit_code)
