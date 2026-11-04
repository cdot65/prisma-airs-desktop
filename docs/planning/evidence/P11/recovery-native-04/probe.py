from pathlib import Path
import argparse,os,sys,json,time,platform
root=Path("/Users/cdot/development/redtail/p09-queue/refinement-20261104/core");os.chdir(root);sys.path.insert(0,str(root/"tools/native-acceptance"))
import acceptance_p11 as core, acceptance_p11_api as api, fixture_app as fx, native_sheets as sheets
p=argparse.ArgumentParser();p.add_argument("--prepare",type=Path,required=True);p.add_argument("--out",type=Path,required=True);a=p.parse_args()
ctx=core.Context(a.prepare,a.out)
ctx.closure=core._binding.helper_closure(root,"acceptance_p11_warn")
ctx.binding["helper_closure"]=ctx.closure
ctx.binding["conditions"]={"platform":platform.platform(),"hostname":platform.node()}
os.environ["AIRS_NATIVE_PROCESS_RECORD"]=str(ctx.out/"candidate-processes.json")
base=root/"docs/planning/evidence/P11/qualification-final-02/qualification.json"
build=root/"docs/planning/evidence/P11/native-build-08-vm/receipt.json"
assert build.is_file(),"new actual build receipt required"
build_record=json.loads(build.read_text());paths=sorted(build_record["application_inputs_after"])
inputs=lambda:{name:core.sha(root/name) for name in paths}
before=inputs();assert before==build_record["application_inputs_after"]
data=Path(fx.DATA);state_before=(data/"state.json").read_bytes()
original_override=(data/"fixture-open-folder").read_bytes()
original_root=Path(original_override.decode().strip())
owned_old=Path("/var/folders/p7/f_99nxmj7s16qrxbl0cbpz4r0000gn/T/airs-p11-owned-j9e8xcq6")
assert original_root.resolve().is_relative_to(owned_old.resolve()) and original_root.is_dir() and not original_root.is_symlink()
assert original_root.stat().st_uid==os.getuid() and (original_root/"collection.toml").is_file()
ctx.app=fx.App(str(ctx.bundle),str(ctx.work),ctx.log)
assert not ctx.app.candidate.matching(ctx.app.candidate.app_path) and not ctx.app.candidate.matching(ctx.app.candidate.engine_path)
endpoint=api._Endpoint();collection=ctx.work/"collection";missing=ctx.work/"held-missing"
api._stage(collection,endpoint.origin)
manifest=collection/"collection.toml";original_manifest=manifest.read_bytes()
was_sheets=Path(sheets.FLAG).exists();facts={};captures=[];exit_code=1
raw={"schema":"airs-desktop/p11-recovery-observations/1","started_utc":time.strftime("%Y-%m-%dT%H:%M:%SZ",time.gmtime()),"argv":[sys.executable,*sys.argv],"cwd":str(root),"host":platform.platform(),"conditions":ctx.binding["conditions"],"prepare":{"path":str(a.prepare),"sha256":core.sha(a.prepare)},"baseline_qualification":{"path":str(base.relative_to(root)),"sha256":core.sha(base)},"build":{"path":str(build.relative_to(root)),"sha256":core.sha(build)},"source_sha256":core.sha(__file__),"executable_sha256":ctx.prepared["executable_sha256"],"bundle_sha256":ctx.prepared["bundle_sha256"],"helper_closure":ctx.closure,"application_inputs_before":before,"original_owned_collection_root":str(original_root),"original_picker_override_sha256":core.sha(data/"fixture-open-folder")}
def close_any():
 tree=ctx.app.tree();node=ctx.app.node(tree,"AXPopUpButton","Collection ")
 if node:api._close_collection(ctx,node["label"][len("Collection "):])
def warning(name):
 tree=ctx.text("The preferred collection could not be opened.")
 assert ctx.app.node(tree,"AXButton","Try again") is not None
 assert not ctx.app.has_text(tree,"193 requests")
 ctx.save(name,tree)
 return tree
def away_back(name):
 ctx.navigate("Home");ctx.navigate("API Client")
 return warning(name)
try:
 sheets.disable();ctx.app.launch();ctx.navigate("API Client")
 close_any();api._open_collection(ctx,data,collection)
 ctx.app.quit();collection.rename(missing)
 ctx.app.launch();ctx.navigate("API Client");warning("missing-restore")
 facts["missing_restore_warning_visible"]=not collection.exists()
 away_back("missing-after-home-back")
 facts["recovery_survives_home_back"]=True
 captures.append(ctx.capture("missing-after-home-back"))
 missing.rename(collection)
 warning("restored-folder-before-explicit-retry")
 facts["retry_is_explicit"]=True
 api._press(ctx,"Try again");ctx.text("Collection "+api.COLLECTION_NAME)
 tree=ctx.app.tree();facts["retry_recovers_when_root_restored"]=not ctx.app.has_text(tree,"The preferred collection could not be opened.")
 ctx.save("missing-explicit-retry-recovered",tree)
 ctx.app.quit()
 changed=original_manifest.replace(api.COLLECTION_ID.encode(),b"p11-recovery-replaced-id")
 assert changed!=original_manifest;manifest.write_bytes(changed)
 ctx.app.launch();ctx.navigate("API Client");warning("replaced-restore")
 facts["replaced_restore_warning_visible"]=manifest.read_bytes()==changed
 away_back("replaced-after-home-back")
 facts["replaced_recovery_survives_home_back"]=True
 captures.append(ctx.capture("replaced-after-home-back"))
 manifest.write_bytes(original_manifest)
 api._press(ctx,"Try again");ctx.text("Collection "+api.COLLECTION_NAME)
 facts["retry_recovers_identity_restored"]=not ctx.app.has_text(ctx.app.tree(),"The preferred collection could not be opened.")
 close_any();ctx.navigate("Home");ctx.navigate("API Client")
 tree=ctx.text("No collection is open.")
 closed0=not ctx.app.has_text(tree,"The preferred collection could not be opened.") and ctx.app.node(tree,"AXButton","Try again") is None
 ctx.app.quit();ctx.app.launch();ctx.navigate("API Client")
 tree=ctx.text("No collection is open.")
 facts["intentional_close_stays_closed"]=closed0 and not ctx.app.has_text(tree,"The preferred collection could not be opened.") and ctx.app.node(tree,"AXButton","Try again") is None
 ctx.save("intentional-closed-after-restart",tree);captures.append(ctx.capture("intentional-closed-after-restart"))
 api._open_collection(ctx,data,original_root)
 facts["original_owned_collection_reopened"]=ctx.app.has_text(ctx.app.tree(),"Collection "+api.COLLECTION_NAME)
 facts["no_request_replayed"]=len(endpoint.requests())==0
 assert all(facts.values()),facts
 core.verify_prepare(a.prepare)
 assert core._binding.helper_closure(root,"acceptance_p11_warn")==ctx.closure
 exit_code=0
except BaseException as error:
 raw["error"]=type(error).__name__+": "+str(error)
 try:ctx.save("failure-tree")
 except Exception:pass
finally:
 if missing.exists() and not collection.exists():missing.rename(collection)
 if manifest.exists():manifest.write_bytes(original_manifest)
 try:ctx.app.quit()
 finally:endpoint.stop()
 sheets.enable() if was_sheets else sheets.disable()
 after=inputs()
 raw.update(exit_code=exit_code,finished_utc=time.strftime("%Y-%m-%dT%H:%M:%SZ",time.gmtime()),facts=facts,application_inputs_after=after,source_inputs_unchanged=before==after,core_fixture_state_unchanged=(data/"state.json").read_bytes()==state_before,original_folder_override_restored=(data/"fixture-open-folder").read_bytes()==original_override,captures=[{"path":name,"sha256":core.sha(root/name)} for name in captures],actions=ctx.log,artifacts=ctx.artifacts,endpoint_requests=endpoint.requests())
 core.write_new(ctx.out/"observations.json",raw)
 assert before==after,"application inputs changed during native recovery"
raise SystemExit(exit_code)
