"""One guest Terminal-owned C45 generation; independent of model/tool lifetime.

Sealed source, global UI lock, exact candidate/foreground guards, immutable
per-stage raw results and fail-stop. Re-entry validates completed stages and
continues at the first unfinished stage; it never starts a second UI writer.
"""
from pathlib import Path
import datetime,fcntl,json,os,platform,shutil,subprocess,sys,time,traceback
ROOT=Path(__file__).resolve().parents[2]
os.chdir(ROOT)
sys.path.insert(0,str(Path(__file__).resolve().parent))
from candidate import verify_source,verify_candidate,helper_closure,bundle_hash
from qualification import read,sha,require,digest
from seal import DRIVERS,LEGACY,verify_plan
from stages import collect,verify_completed,verify_conditions
BASE=ROOT/'docs/planning/evidence/C45/baseline-02'
RUN=ROOT/'.evidence-private/worf-agent/2026-11-02-c45-p10-01'
ENV=os.environ.copy()
ENV['PATH']=str(ROOT/'.tools/bin')+':'+str(Path.home()/'.cargo/bin')+':'+str(Path.home()/'.nvm/versions/node/v24.21.0/bin')+':/usr/bin:/bin:/usr/sbin:/sbin'
for key in ['RUSTFLAGS','RUSTDOCFLAGS','CARGO_ENCODED_RUSTFLAGS','CARGO_TARGET_DIR','CARGO_BUILD_TARGET','NODE_OPTIONS','NODE_PATH']:ENV.pop(key,None)
ENV['RUST_TEST_THREADS']='4'
ENV['AIRS_NATIVE_ORCHESTRATOR']='Codex'
ENV['PYTHONDONTWRITEBYTECODE']='1'


def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()


def write(path,value):
    path.parent.mkdir(parents=True,exist_ok=True);temp=path.with_name(path.name+'.tmp');temp.write_text(json.dumps(value,indent=2)+'\n');temp.replace(path)


def status(state,**extra):
    value={'time':now(),'state':state,'pid':os.getpid(),'worktree':str(ROOT),**extra}
    write(RUN/'qualification-status.json',value)
    with (RUN/'qualification-events.jsonl').open('a') as stream:stream.write(json.dumps(value)+'\n')
    print(json.dumps(value),flush=True)


def ancestry():
    rows={int(parts[0]):(int(parts[1]),parts[2]) for line in subprocess.check_output(['/bin/ps','-axo','pid=,ppid=,comm='],text=True).splitlines() if len(parts:=line.strip().split(None,2))==3}
    parent=os.getppid();chain=[]
    while parent in rows and parent>1:
        pp,command=rows[parent];chain.append(dict(pid=parent,ppid=pp,command=command));parent=pp
    require(any('Terminal.app/Contents/MacOS/Terminal' in item['command'] for item in chain),'qualification must be guest Terminal-owned')
    return chain


def foreground():
    value=subprocess.check_output(['/usr/bin/osascript','-e','tell application "System Events" to get bundle identifier of first application process whose frontmost is true'],text=True).strip()
    require(value in ['com.apple.Terminal','io.cdot.airs-desktop.fixture'],'foreign foreground application: '+value)
    return value


def source_guard():return verify_source(ROOT,'docs/planning/evidence/C45/baseline-02/source-seal.json')


def execute(stage,argv,candidate=None):
    source=source_guard();folder=BASE/'stages'/stage;folder.mkdir(parents=True,exist_ok=True)
    done=folder/'completed.json'
    if done.exists():
        old=read(done)
        verify_completed(ROOT,folder,old,argv,source,sha(BASE/'candidate.json') if candidate else None)
        status('retained valid completed stage',stage=stage);return old
    require(not (folder/'started.json').exists(),'partial prior stage; inspect actual ownership/results before recovery')
    if candidate:
        verify_candidate(ROOT,candidate)
        require(candidate['conditions']['platform']==platform.platform(),'execution platform/conditions changed; diagnose affected evidence')
    started=now();write(folder/'started.json',{'argv':argv,'started_utc':started,'source_fingerprint':source,'supervisor_pid':os.getpid(),'foreground':foreground()})
    status('executing actual stage',stage=stage,argv=argv)
    with (folder/'stdout.log').open('x') as stdout,(folder/'stderr.log').open('x') as stderr:
        child=subprocess.Popen(argv,cwd=ROOT,env=ENV,stdin=subprocess.DEVNULL,stdout=stdout,stderr=stderr)
        write(RUN/'active-stage.json',{'stage':stage,'pid':child.pid,'supervisor_pid':os.getpid(),'started_utc':started,'argv':argv})
        code=child.wait()
    require(source_guard()==source,'source changed while stage ran')
    result={'execution_id':stage+'-'+started,'argv':argv,'exit_code':code,'source_fingerprint':source,'started_utc':started,'finished_utc':now(),'stdout_sha256':sha(folder/'stdout.log'),'stderr_sha256':sha(folder/'stderr.log')}
    if candidate:result['candidate_sha256']=sha(BASE/'candidate.json')
    if stage.startswith('command-') and code==0:
        raw=read(folder/'stdout.log');require(raw['schema']=='airs-v2/run/1' and raw['status']=='passed','existing runner did not return passing raw RunRecord')
        raw_path=ROOT/'docs/planning/evidence/C45/runs'/raw['run_id']/'run.json'
        require(read(raw_path)==raw,'runner stdout differs from immutable original run')
        result['raw']=str(raw_path.relative_to(ROOT))
    if code==0:result['artifacts']=collect(ROOT,BASE,stage,result.get('raw'))
    write(folder/('completed.json' if code==0 else 'failed.json'),result)
    status('actual stage passed' if code==0 else 'actual stage failed; STOP',stage=stage,exit_code=code)
    require(code==0,'stage failed; preserve raw results, diagnose affected change only')
    return result


def main():
    BASE.mkdir(exist_ok=True)
    lock=open('/private/tmp/prisma-airs-desktop-native-writer.lock','a+')
    fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
    chain=ancestry();source=source_guard();plan=read(BASE/'required-coverage.json');verify_plan(ROOT,plan)
    owner=RUN/'qualification-owner.json'
    if owner.exists():require(read(owner)['supervisor_sha256']==sha(__file__),'supervisor changed during recovery')
    write(owner,{'pid':os.getpid(),'time':now(),'source_fingerprint':source,'supervisor_sha256':sha(__file__),'terminal_ancestry':chain,'global_ui_lock':'/private/tmp/prisma-airs-desktop-native-writer.lock'})
    status('C45 one-generation qualification active',source_fingerprint=source,foreground=foreground())
    execute('build',['cargo','run','--locked','-p','xtask','--','native-prepare','--chunk','C45','--target','aarch64-apple-darwin','--features','test-fixtures'])
    if not (BASE/'native-prepare.json').exists():shutil.copy2(ROOT/'docs/planning/evidence/C45/native-prepare.json',BASE/'native-prepare.json')
    if not (BASE/'candidate.json').exists():
        prepared=read(BASE/'native-prepare.json')
        fp=json.loads(subprocess.check_output(['cargo','run','--locked','-q','-p','xtask','--example','c45_baseline','--','identity'],cwd=ROOT,env=ENV,text=True))
        candidate={'schema':'airs-desktop/candidate/1','execution_root':str(ROOT),'source_inventory_sha256':source,'executable_sha256':prepared['executable_sha256'],'bundle_sha256':prepared['bundle_sha256'],'prepare':{'path':str((BASE/'native-prepare.json').relative_to(ROOT)),'sha256':sha(BASE/'native-prepare.json')},'helper_closure':plan['helper_closure'],'build_flags':{'features':prepared['features'],'target':prepared['target'],'commands':prepared['commands'],'manifest_sha256':sha(ROOT/'docs/architecture/checks.json')},'conditions':{'platform':platform.platform(),'hostname':platform.node(),'guest':'Worf VM','synthetic_only':True,'RUST_TEST_THREADS':'4','fixture_identifier':prepared['identifier'],'ui_writer':'one guest Terminal-owned exact-PID/birth fixture driver'},'dependency_fingerprint':fp['dependencies'],'runner_inputs':{k:fp[k] for k in ['source','dependencies','build_flags']}}
        verify_candidate(ROOT,candidate);write(BASE/'candidate.json',candidate)
    candidate=read(BASE/'candidate.json');verify_candidate(ROOT,candidate)
    current=json.loads(subprocess.check_output(['cargo','run','--locked','-q','-p','xtask','--example','c45_baseline','--','identity'],cwd=ROOT,env=ENV,text=True))
    verify_conditions(candidate,current,platform.platform(),platform.node())
    ENV['AIRS_NATIVE_PREPARE']=str(BASE/'native-prepare.json');ENV['AIRS_NATIVE_PROCESS_RECORD']=str(BASE/'startup-smoke-owned-processes.json')
    execute('startup-smoke',['python3','tools/native-acceptance/acceptance_c45.py','--smoke'],candidate)
    for command in plan['commands']:
        kind='check' if command['label'].startswith('common/') else 'test'
        execute('command-'+command['label'].replace('/','-'),['cargo','run','--locked','-q','-p','xtask','--example','c45_baseline','--',kind,command['label']],candidate)
    for driver in DRIVERS:
        execute('native-'+driver,['python3','tools/p10/native.py',driver]+(['legacy'] if driver in LEGACY else []),candidate)
    ENV['AIRS_NATIVE_PROCESS_RECORD']=str(BASE/'identity-owned-processes.json')
    execute('native-c45',['python3','tools/native-acceptance/acceptance_c45.py'],candidate)
    execute('finalize',['python3','tools/p10/finalize.py'],candidate)
    status('C45 baseline qualified; actual independent C45/P10 reviews remain',qualified_coverage='docs/planning/evidence/C45/qualified-coverage.json',source_fingerprint=source)


if __name__=='__main__':
    try:main()
    except Exception:
        status('FAIL-STOP; inspect raw stage and ownership before recovery',traceback=traceback.format_exc())
        raise
