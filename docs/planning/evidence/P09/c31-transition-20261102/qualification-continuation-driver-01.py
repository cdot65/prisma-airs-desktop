"""Guest Terminal-owned one-generation qualification; no C30 work or review scores.

Run only after transition sealing. Re-entry preserves completed valid stages.
Failures stop; incomplete driver evidence requires explicit diagnosis/archival.
"""
from pathlib import Path
import datetime,fcntl,hashlib,json,os,shutil,subprocess,sys,traceback
ROOT=Path('/Users/cdot/development/redtail/prisma-airs-desktop-v2-current')
os.chdir(ROOT)
sys.path.insert(0,str(ROOT/'tools/p09'))
import c31_transition_policy as policy
import phase_inputs_c31_transition as inputs
RUN=ROOT/'.evidence-private/worf-agent/2026-11-02-c31-integration-01'
PHASE=ROOT/'docs/planning/evidence/P09'
GEN=PHASE/'preparations/worf-08-c31'
BASE=Path('/Users/admin/.agent-workflows/airs/2026-11-01-worf-start')
ENV=os.environ.copy();ENV['PATH']=str(ROOT/'.tools/bin')+':/Users/admin/.cargo/bin:/Users/admin/.nvm/versions/node/v24.21.0/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin'
for key in ['RUSTFLAGS','CARGO_BUILD_RUSTFLAGS','CARGO_TARGET_DIR']:ENV.pop(key,None)
os.environ.update(ENV)
def read(p):return json.loads(Path(p).read_text())
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def write(p,v):Path(p).write_text(json.dumps(v,indent=2)+'\n')
def status(state,**extra):
 value={'time':now(),'supervisor_pid':os.getpid(),'author_pid':25361,'state':state,**extra}
 write(RUN/'qualification-status.json',value)
 (RUN/'runstatus.md').write_text(json.dumps(value,indent=2)+'\n')
 with (RUN/'qualification-events.jsonl').open('a') as f:f.write(json.dumps(value)+'\n')
 pointer=BASE/'active-agent.json';x=read(pointer);x.update(time=value['time'],state=state,qualification_supervisor_pid=os.getpid(),run_dir=str(RUN));write(pointer,x)
 print(json.dumps(value),flush=True)
def command(cmd,path):
 path=Path(path);assert not path.exists(),'retain existing command log '+str(path)
 with path.open('x') as log:rc=subprocess.run(cmd,cwd=ROOT,env=ENV,stdin=subprocess.DEVNULL,stdout=log,stderr=subprocess.STDOUT).returncode
 assert rc==0,'command failed '+str(cmd)+' exit '+str(rc)
 return {'argv':cmd,'cwd':str(ROOT),'exit':rc,'log':str(path.relative_to(ROOT)),'log_sha256':sha(path)}
def source_guard():
 policy.verify_transition(ROOT)
 assert subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip()==owner['source_head'],'source HEAD changed while sealed qualification owns it'
def foreground_guard():
 text=subprocess.check_output(['/usr/bin/osascript','-e','tell application "System Events" to get bundle identifier of first application process whose frontmost is true'],text=True).strip()
 assert text in ['com.apple.Terminal','com.apple.finder','io.cdot.prisma-airs-desktop.v2.fixture'],'unrelated foreground app '+text
 return text
def inventory(path):
 return subprocess.check_output(['/bin/bash',str(PHASE/'runs/hash-inputs.sh'),str(ROOT),str(path)],text=True).strip()
def bundle_hash(bundle):
 entries=[]
 for p in sorted(bundle.rglob('*')):
  if p.is_dir() and not p.is_symlink():continue
  rel=str(p.relative_to(bundle))
  if p.is_symlink():mode='symlink';h=hashlib.sha256(os.readlink(p).encode()).hexdigest()
  else:mode='100755' if p.stat().st_mode & 0o111 else '100644';h=sha(p)
  entries.append(f'{rel}\0{mode}\0{h}\n')
 return hashlib.sha256(''.join(entries).encode()).hexdigest()
def artifact_guard():
 p=read(PHASE/'native-prepare.json');bundle=ROOT/p['bundle']
 assert sha(bundle/'Contents/MacOS/prisma-airs-desktop-v2')==p['executable_sha256']
 assert bundle_hash(bundle)==p['bundle_sha256'],'full bundle drift'
 return p
lock=(RUN/'qualification.lock').open('a+')
fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
owner_path=RUN/'qualification-driver-01-owner.json'
if owner_path.exists():
 owner=read(owner_path)
 assert owner['supervisor_source_sha256']==sha(__file__),'supervisor source changed'
else:
 # Exact Terminal ancestry is recorded, not inferred from an orphan process.
 ancestry=subprocess.check_output(['/bin/ps','-axo','pid=,ppid=,command='],text=True)
 parent=os.getppid();chain=[]
 rows={int(v[0]):(int(v[1]),v[2]) for line in ancestry.splitlines() if len(v:=line.strip().split(None,2))==3}
 while parent in rows and parent>1:
  pp,cmd=rows[parent];chain.append({'pid':parent,'ppid':pp,'command':cmd});parent=pp
 assert any('Terminal.app/Contents/MacOS/Terminal' in x['command'] for x in chain),'not Terminal-owned'
 owner={'created_utc':now(),'source_head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),'supervisor_source_sha256':sha(__file__),'terminal_ancestry':chain,'transition':policy.verify_transition(ROOT)}
 write(owner_path,owner)
try:
 source_guard();status('G8 driver-only continuation active; ten completed steps retained under explicit same-bundle proof; C31/C39 next; no C30 work',foreground=foreground_guard())
 # Existing G7 is preserved twice: original complete archive, then this exact
 # canonical view before paths are reused by the established controller.
 archive=PHASE/'native-history/Worf-generation-07-canonical-before-C31-20261102'
 if not archive.exists():
  archive.mkdir()
  for name in ['native','parity','screens','native-acceptance.json','execution.json','native-prepare.json','native-build-inputs.json']:
   p=PHASE/name
   if p.exists():shutil.move(str(p),str(archive/name))
  write(archive/'archive.json',{'time':now(),'meaning':'Exact historical canonical G7 view moved before C31 generation; original G7 complete archive unchanged','files':{str(p.relative_to(archive)):sha(p) for p in archive.rglob('*') if p.is_file()}})
 if not (GEN/'build-inputs.json').exists():
  GEN.mkdir(exist_ok=True)
  before=inventory(GEN/'inputs-before.sha256')
  with (GEN/'fixture-remediation.diff').open('x') as f:subprocess.run(['git','diff','HEAD','--','apps','crates','tools/native-acceptance'],stdout=f,check=True)
  status('G8 current native preparation running; C31 production lifecycle changed')
  cmd=['cargo','run','--locked','-p','xtask','--','native-prepare','--phase','P09','--target','aarch64-apple-darwin','--features','test-fixtures']
  record=command(cmd,GEN/'native-prepare.log')
  after=inventory(GEN/'inputs-after.sha256');assert before==after
  prep=artifact_guard();source_guard()
  build={'schema':'airs-v2/p09-native-build-inputs/1','generation':'worf-08-c31','source_head':owner['source_head'],'worktree_diff':str((GEN/'fixture-remediation.diff').relative_to(ROOT)),'worktree_diff_sha256':sha(GEN/'fixture-remediation.diff'),'prepare':'docs/planning/evidence/P09/native-prepare.json','prepare_sha256':sha(PHASE/'native-prepare.json'),'executable_sha256':prep['executable_sha256'],'bundle_sha256':prep['bundle_sha256'],'inputs':{'procedure':'docs/planning/evidence/P09/runs/hash-inputs.sh','procedure_sha256':sha(PHASE/'runs/hash-inputs.sh'),'before_inventory':str((GEN/'inputs-before.sha256').relative_to(ROOT)),'after_inventory':str((GEN/'inputs-after.sha256').relative_to(ROOT)),'before_digest':before,'after_digest':after,'unchanged':True,'members':len((GEN/'inputs-before.sha256').read_text().splitlines())},'command':record,'prior_generation':str(archive.relative_to(ROOT)),'qualification':'Current C31 changed production bytes; all19 native drivers and18 checks required. Historical C30 timing unchanged, no current timing claim.'}
  write(GEN/'build-inputs.json',build);write(PHASE/'native-build-inputs.json',build)
 else:
  artifact_guard();inputs.native_build(ROOT,read(PHASE/'native-prepare.json'))
 results_path=RUN/'native-08-results.json';results=read(results_path) if results_path.exists() else []
 queue=[('c36',False),('c43',False)]+[(x,True) for x in ['c11','c10','c03','c04','c05','c06','c07','c24','c31']]+[(x,False) for x in ['c39','c35','c37','c38','c40','c41','c42','c44']]
 for which,legacy in queue:
  source_guard();artifact_guard()
  prior=next((x for x in results if x['subject']==which.upper()),None)
  directory=PHASE if which=='c44' else PHASE/'native/cumulative'/which.upper()
  if prior:
   assert prior['exit']==0 and sha(ROOT/prior['log'])==prior['log_sha256'] and sha(directory/'execution.json')==prior['execution_sha256'] and sha(directory/'native-acceptance.json')==prior['raw_record_sha256'],'completed step drift'
   continue
  foreground=foreground_guard();status('G8 '+which.upper()+' running; sole native UI writer',subject=which.upper(),foreground=foreground)
  cmd=[sys.executable,'-I',str(PHASE/'native-controller/run-p09-native.py'),which]+(['legacy'] if legacy else [])
  started=now();record=command(cmd,RUN/('native-08-'+which+('-driver01' if which=='c31' else '')+'.log'))
  execution=read(directory/'execution.json');assert execution['required_check_errors']==[]
  record.update(subject=which.upper(),started_utc=started,finished_utc=now(),execution_sha256=sha(directory/'execution.json'),raw_record_sha256=sha(directory/'native-acceptance.json'))
  results.append(record);write(results_path,results);status('G8 '+which.upper()+' passed actual required native checks',completed=len(results))
 source_guard();artifact_guard();qualified=inputs.verify_native(ROOT);write(RUN/'native-08-audit.json',qualified)
 check=PHASE/'runs/run-attempt-08.json'
 if not check.exists():
  status('G8 all19 native drivers passed; current repository18 checks running once')
  command(['/bin/bash',str(PHASE/'runs/run-checks.sh'),'attempt-08'],RUN/'checks-08.log')
 if not (PHASE/'runs/attempt-08/post-check-audit.json').exists():
  command([sys.executable,'-I','tools/p09/audit-p09-checks-c31-transition.py','attempt-08'],RUN/'audit-checks-08.log')
 source_guard();artifact_guard();status('G8 all19 native drivers and current18 repository checks passed and audited; fresh independent C31attempt06 then C32attempt21 and P09 required',complete=True)
 write(RUN/'qualification-complete.json',{'time':now(),'current_native_audit':str((RUN/'native-08-audit.json').relative_to(ROOT)),'checks':'docs/planning/evidence/P09/runs/run-attempt-08.json','source_head':owner['source_head']})
except BaseException as error:
 status('Qualification stopped on actual condition; completed results retained; no automatic retry',error=str(error),traceback=traceback.format_exc())
 raise
