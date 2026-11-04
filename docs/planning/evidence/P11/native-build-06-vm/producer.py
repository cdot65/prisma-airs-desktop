from pathlib import Path
import os,sys,json,subprocess,hashlib,datetime,fcntl
root=Path.cwd()
out=root/"docs/planning/evidence/P11/native-build-06-vm"
out.mkdir(exist_ok=False)
lock=open("/private/tmp/prisma-airs-desktop-native-writer.lock","a+");fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
now=lambda:datetime.datetime.now(datetime.timezone.utc).isoformat()
prior=root/"docs/planning/evidence/P11/native-prepare.json";old_sha=sha(prior)
old=json.loads((root/"docs/planning/evidence/P11/native-build-04-vm/receipt.json").read_text())
paths=sorted(old["application_inputs_after"])
inputs=lambda:{p:sha(root/p) for p in paths}
spec=json.loads((root/"docs/architecture/toolchain.json").read_text())
env={k:os.environ[k] for k in spec["preserve_unchanged"] if k in os.environ};env.update(spec["declared_env"])
env["PATH"]=":".join(p.replace("{repo}",str(root)).replace("$HOME",str(Path.home())) for p in spec["path_dirs"])
argv=["cargo","run","--offline","--locked","-p","xtask","--example","p11_native_candidate","--","--out","docs/planning/evidence/P11/native-build-06-vm/native-prepare.json"]
receipt={"schema":"airs-desktop/p11-versioned-build/1","host":os.uname().nodename,"started_utc":now(),"head":subprocess.check_output(["git","rev-parse","HEAD"],text=True).strip(),"purpose":"One affected API recovery fixture build using the existing fixed native recipe and versioned preparation. No check/test/native matrix is executed by this command.","source":{"path":".evidence-private/native-build-06-vm/run.py","sha256":sha(__file__)},"adapter":{"path":"crates/xtask/examples/p11_native_candidate.rs","sha256":sha(root/"crates/xtask/examples/p11_native_candidate.rs")},"canonical_prepare_before_sha256":old_sha,"application_inputs_before":inputs(),"steps":[]}
(out/"receipt.json").write_text(json.dumps(receipt,indent=2)+"\n")
with (out/"build.log").open("w") as log:
 result=subprocess.run(argv,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
receipt["steps"].append({"argv":argv,"exit_code":result.returncode,"log":"build.log","log_sha256":sha(out/"build.log")})
receipt.update(finished_utc=now(),application_inputs_after=inputs(),canonical_prepare_after_sha256=sha(prior))
receipt["application_inputs_unchanged"]=receipt["application_inputs_before"]==receipt["application_inputs_after"]
receipt["canonical_prepare_unchanged"]=old_sha==receipt["canonical_prepare_after_sha256"]
if result.returncode==0:
 prepared=json.loads((out/"native-prepare.json").read_text())
 receipt.update(executable_sha256=prepared["executable_sha256"],bundle_sha256=prepared["bundle_sha256"],prepare={"path":str((out/"native-prepare.json").relative_to(root)),"sha256":sha(out/"native-prepare.json")})
(out/"receipt.json").write_text(json.dumps(receipt,indent=2)+"\n")
assert result.returncode==0 and receipt["application_inputs_unchanged"] and receipt["canonical_prepare_unchanged"]
print(json.dumps({k:receipt[k] for k in ("finished_utc","executable_sha256","bundle_sha256","application_inputs_unchanged","canonical_prepare_unchanged")}),flush=True)
