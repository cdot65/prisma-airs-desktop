from pathlib import Path
import datetime,fcntl,hashlib,json,os,subprocess,sys,traceback
ROOT=Path('/Users/cdot/development/redtail/prisma-airs-desktop-v2-current');os.chdir(ROOT)
RUN=ROOT/'.evidence-private/worf-agent/2026-11-02-c32-remediation-03'
EVIDENCE='docs/planning/evidence/C32/remediation-attempt-21/native-attempt-03'
sys.path.insert(0,str(ROOT/'tools/p09'))
from phase_packet_owner_closure import source_inventory,digest,file_identity
lock=(RUN/'native.lock').open('a+');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
def read(p):return json.loads(p.read_text())
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def status(s,**kw):
 x={'time':datetime.datetime.now(datetime.timezone.utc).isoformat(),'pid':os.getpid(),'state':s,**kw};(RUN/'runstatus.md').write_text(json.dumps(x,indent=2)+'\n');print(json.dumps(x),flush=True)
def guard():
 seal=read(RUN/'source-seal.json');assert source_inventory(ROOT)==seal['files'];p=read(ROOT/EVIDENCE/'native-prepare.json');b=ROOT/p['bundle'];assert sha(b/'Contents/MacOS/prisma-airs-desktop-v2')==p['executable_sha256'];
 return p
try:
 rows={int(v[0]):(int(v[1]),v[2]) for l in subprocess.check_output(['ps','-axo','pid=,ppid=,command='],text=True).splitlines() if len(v:=l.strip().split(None,2))==3};n=os.getppid();chain=[]
 while n in rows and n>1:
  pp,cmd=rows[n];chain.append({'pid':n,'ppid':pp,'command':cmd});n=pp
 assert any('Terminal.app/Contents/MacOS/Terminal' in x['command'] for x in chain)
 p=guard();bundle=ROOT/p['bundle'];before={str(q.relative_to(bundle)):file_identity(q) for q in bundle.rglob('*') if q.is_file() or q.is_symlink()}
 foreground=subprocess.check_output(['osascript','-e','tell application "System Events" to get bundle identifier of first application process whose frontmost is true'],text=True).strip();assert foreground in ('com.apple.Terminal','com.apple.finder',p['identifier'])
 (RUN/'owner.json').write_text(json.dumps({'pid':os.getpid(),'terminal_ancestry':chain,'source_seal_sha256':sha(RUN/'source-seal.json'),'prepared':p,'bundle_inventory':before},indent=2)+'\n')
 status('C32-only native coverage running on exact G9 bundle; sole guest UI writer; no C30 scenario')
 env=os.environ.copy();env['AIRS_C32_EVIDENCE']=EVIDENCE
 with (RUN/'native-attempt-03.log').open('x') as f:rc=subprocess.run([sys.executable,'-I','tools/native-acceptance/acceptance_c32_followup.py'],env=env,stdout=f,stderr=subprocess.STDOUT).returncode
 guard();assert before=={str(q.relative_to(bundle)):file_identity(q) for q in bundle.rglob('*') if q.is_file() or q.is_symlink()}
 status('C32 native driver finished; inspect actual checks',exit=rc)
 (RUN/'result.json').write_text(json.dumps({'exit':rc,'evidence':EVIDENCE,'log_sha256':sha(RUN/'native-attempt-03.log')},indent=2)+'\n')
except BaseException as e:
 status('C32 native job stopped',error=str(e),traceback=traceback.format_exc());raise
