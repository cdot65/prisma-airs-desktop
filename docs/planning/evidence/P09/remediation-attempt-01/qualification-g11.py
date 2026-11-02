"""Guest Terminal-owned one-generation qualification; no C30 work or review scores.

Run only after transition sealing. Re-entry preserves completed valid stages.
Failures stop; incomplete driver evidence requires explicit diagnosis/archival.
"""
from pathlib import Path
import datetime,fcntl,hashlib,json,os,shutil,subprocess,sys,traceback
ROOT=Path('/Users/cdot/development/redtail/prisma-airs-desktop-v2-current')
os.chdir(ROOT)
sys.path.insert(0,str(ROOT/'tools/p09'))
import p09_workbench_policy as policy
import phase_inputs_p09_workbench as inputs
RUN=ROOT/'.evidence-private/worf-agent/2026-11-02-p09-phase-remediation-01'
PHASE=ROOT/'docs/planning/evidence/P09'
GEN=PHASE/'preparations/worf-11-p09-workbench-lifetime'
BASE=Path('/Users/admin/.agent-workflows/airs/2026-11-01-worf-start')
ENV=os.environ.copy();ENV['PATH']=str(ROOT/'.tools/bin')+':/Users/admin/.cargo/bin:/Users/admin/.nvm/versions/node/v24.21.0/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin'
for key in ['RUSTFLAGS','CARGO_BUILD_RUSTFLAGS','CARGO_TARGET_DIR']:ENV.pop(key,None)
os.environ.update(ENV)
def read(p):return json.loads(Path(p).read_text())
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def write(p,v):Path(p).write_text(json.dumps(v,indent=2)+'\n')
def status(state,**extra):
 value={'time':now(),'supervisor_pid':os.getpid(),'author_pid':57529,'state':state,**extra}
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
owner_path=RUN/'qualification-owner.json'
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
 source_guard();status('G11 C32 production qualification supervisor active; source sealed; no C30 qualification',foreground=foreground_guard())
 # Existing G8 is preserved twice: original complete archive, then this exact
 # canonical view before paths are reused by the established controller.
 archive=PHASE/'native-history/Worf-generation-10-complete-before-P09-workbench-lifetime-20261102'
 relocation=archive/'canonical-original-paths'
 if not relocation.exists():
  assert (archive/'application').is_dir() and (archive/'archive.json').is_file(),'G10 exact full archive must exist before rebuild'
  relocation.mkdir()
  for name in ['native','parity','screens','native-acceptance.json','execution.json','native-prepare.json','native-build-inputs.json']:
   p=PHASE/name
   if p.exists():shutil.move(str(p),str(relocation/name))
  write(relocation/'relocation.json',{'time':now(),'meaning':'Original G10 canonical paths moved intact before G11. Full G10 archive copy and original reviews unchanged.','files':{str(p.relative_to(relocation)):sha(p) for p in relocation.rglob('*') if p.is_file()}})
 if not (GEN/'build-inputs.json').exists():
  GEN.mkdir(exist_ok=True)
  before=inventory(GEN/'inputs-before.sha256')
  with (GEN/'fixture-remediation.diff').open('x') as f:subprocess.run(['git','diff','HEAD','--','apps','crates','tools/native-acceptance'],stdout=f,check=True)
  status('G11 current native preparation running; P09 Workbench native-confirmation ownership changed')
  cmd=['cargo','run','--locked','-p','xtask','--','native-prepare','--phase','P09','--target','aarch64-apple-darwin','--features','test-fixtures']
  record=command(cmd,GEN/'native-prepare.log')
  after=inventory(GEN/'inputs-after.sha256');assert before==after
  prep=artifact_guard();source_guard()
  assert prep['executable_sha256'] != 'd8bf6a0f91507b6643a9888169873fe5c8bc1c96739bc2cd33b61aa3805ebcca', 'final production fix must not silently reuse the historical G10 executable'
  build={'schema':'airs-v2/p09-native-build-inputs/1','generation':'worf-11-p09-workbench-lifetime','source_head':owner['source_head'],'worktree_diff':str((GEN/'fixture-remediation.diff').relative_to(ROOT)),'worktree_diff_sha256':sha(GEN/'fixture-remediation.diff'),'prepare':'docs/planning/evidence/P09/native-prepare.json','prepare_sha256':sha(PHASE/'native-prepare.json'),'executable_sha256':prep['executable_sha256'],'bundle_sha256':prep['bundle_sha256'],'inputs':{'procedure':'docs/planning/evidence/P09/runs/hash-inputs.sh','procedure_sha256':sha(PHASE/'runs/hash-inputs.sh'),'before_inventory':str((GEN/'inputs-before.sha256').relative_to(ROOT)),'after_inventory':str((GEN/'inputs-after.sha256').relative_to(ROOT)),'before_digest':before,'after_digest':after,'unchanged':True,'members':len((GEN/'inputs-before.sha256').read_text().splitlines())},'command':record,'prior_generation':str(archive.relative_to(ROOT)),'qualification':'Current P09 Workbench ownership changed production bytes; all20 mapped native drivers and18 checks required. Historical C30 timing unchanged, no current timing claim.'}
  write(GEN/'build-inputs.json',build);write(PHASE/'native-build-inputs.json',build)
 else:
  artifact_guard();inputs.native_build(ROOT,read(PHASE/'native-prepare.json'))
 results_path=RUN/'native-11-results.json';results=read(results_path) if results_path.exists() else []
 queue=[('c36',False),('c43',False),('c32',False),('c31',True),('c39',False)]+[(x,True) for x in ['c11','c10','c03','c04','c05','c06','c07','c24']]+[(x,False) for x in ['c35','c37','c38','c40','c41','c42','c44']]
 for which,legacy in queue:
  source_guard();artifact_guard()
  prior=next((x for x in results if x['subject']==which.upper()),None)
  directory=PHASE if which=='c44' else PHASE/'native/cumulative'/which.upper()
  if prior:
   assert prior['exit']==0 and sha(ROOT/prior['log'])==prior['log_sha256'] and sha(directory/'execution.json')==prior['execution_sha256'] and sha(directory/'native-acceptance.json')==prior['raw_record_sha256'],'completed step drift'
   continue
  foreground=foreground_guard();status('G11 '+which.upper()+' running; sole native UI writer',subject=which.upper(),foreground=foreground)
  cmd=[sys.executable,'-I',str(PHASE/'native-controller/run-p09-native-c32.py'),which]+(['legacy'] if legacy else [])
  started=now();record=command(cmd,RUN/('native-11-'+which+'.log'))
  execution=read(directory/'execution.json');assert execution['required_check_errors']==[]
  record.update(subject=which.upper(),started_utc=started,finished_utc=now(),execution_sha256=sha(directory/'execution.json'),raw_record_sha256=sha(directory/'native-acceptance.json'))
  results.append(record);write(results_path,results);status('G11 '+which.upper()+' passed actual required native checks',completed=len(results))
 source_guard();artifact_guard();qualified=inputs.verify_native(ROOT);write(RUN/'native-11-audit.json',qualified)
 check=PHASE/'runs/run-attempt-11.json'
 if not check.exists():
  status('G11 all20 native drivers passed; current repository18 checks running once')
  command(['/bin/bash',str(PHASE/'runs/run-checks.sh'),'attempt-11'],RUN/'checks-11.log')
 if not (PHASE/'runs/attempt-11/post-check-audit.json').exists():
  command([sys.executable,'-I','tools/p09/audit-p09-checks-workbench.py','attempt-11'],RUN/'audit-checks-11.log')
 source_guard();artifact_guard();status('G11 all20 native drivers and current18 repository checks passed and audited; actual P09attempt02 required; C31attempt07/C32attempt23 actualall9 remain closed with no re-review',complete=True)
 write(RUN/'qualification-complete.json',{'time':now(),'current_native_audit':str((RUN/'native-11-audit.json').relative_to(ROOT)),'checks':'docs/planning/evidence/P09/runs/run-attempt-11.json','source_head':owner['source_head']})
except BaseException as error:
 status('Qualification stopped on actual condition; completed results retained; no automatic retry',error=str(error),traceback=traceback.format_exc())
 raise
