from pathlib import Path
import os,sys,json,subprocess,time,fcntl,hashlib,shutil
root=Path(__file__).resolve().parents[2]
folder=Path(__file__).resolve().parent
out=root/'docs/planning/evidence/P11/recovery-native-01/raw'
assert not out.exists(),'preserve previous attempt'
sys.path.insert(0,str(root/'tools/native-acceptance'))
from candidate_process import MacProcesses
from acceptance_p11 import verify_prepare
prepared,bundle=verify_prepare(root/'docs/planning/evidence/P11/native-build-05-vm/native-prepare.json')
lock=open('/private/tmp/prisma-airs-desktop-native-writer.lock','a+')
fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
source_sha=hashlib.sha256(Path(__file__).read_bytes()).hexdigest()
argv=['/usr/bin/python3','.evidence-private/recovery-native-job-01/run.py','--prepare','docs/planning/evidence/P11/native-build-05-vm/native-prepare.json','--out','docs/planning/evidence/P11/recovery-native-01/raw']
env=dict(os.environ)
env['PYTHONUNBUFFERED']='1'
env['AIRS_NATIVE_ORCHESTRATOR']='Codex /root; all native execution on worf-vm admin501 via SSH, owned startup-window activation via agent-desktop; orchestrator sha256 '+source_sha
A='/Users/admin/.nvm/versions/node/v24.21.0/bin/agent-desktop'
state={'host':os.uname().nodename,'state':'starting','argv':argv,'started_utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'orchestrator_sha256':source_sha,'native_assertions_observed':0}
actions=[];focused=set();processes=MacProcesses()
def save():
 p=folder/'state.tmp';p.write_text(json.dumps(state,indent=2)+'\n');p.replace(folder/'state.json')
 if out.is_dir():
  (out/'vm-operator-actions.json').write_text(json.dumps({'host':state['host'],'operator':'Codex /root','orchestrator_sha256':source_sha,'actions':actions},indent=2)+'\n')
def agent(args):
 r=subprocess.run([A,'--session','p11-recovery-01',*args],capture_output=True,text=True,timeout=15)
 value=json.loads(r.stdout) if r.stdout else {'ok':False,'exit_code':r.returncode,'stderr':r.stderr[:600]}
 return value
save()
with (folder/'output.log').open('w') as log:
 proc=subprocess.Popen(argv,cwd=root,env=env,stdout=log,stderr=subprocess.STDOUT)
 state.update(state='running',driver_pid=proc.pid);save()
 print('VM native driver started PID',proc.pid,flush=True)
 last_report=None
 while proc.poll() is None:
  try:
   if out.is_dir() and not (out/'vm-orchestrator.py').exists():
    shutil.copyfile(__file__,out/'vm-orchestrator.py')
   record=out/'candidate-processes.json'
   if record.exists():
    candidate=json.loads(record.read_text()).get('app')
    if candidate:
     identity=processes.snapshot(candidate['pid'])
     key=(candidate['pid'],tuple(candidate['birth']))
     expected=str(bundle/'Contents/MacOS/prisma-airs-desktop')
     owned=(identity and identity.executable==expected and identity.birth==tuple(candidate['birth']) and identity.parent==proc.pid)
     if owned and key not in focused:
      windows=agent(['list-windows','--app','Prisma AIRS Desktop Fixture'])
      rows=windows.get('data',[]) if windows.get('ok') else []
      matches=[r for r in rows if r.get('pid')==identity.pid and r.get('accessible') and r.get('title')=='Prisma AIRS Desktop (Fixture)']
      if len(matches)==1:
       window=matches[0]
       if not window.get('is_focused'):
        latest=processes.snapshot(identity.pid)
        if latest!=identity:raise RuntimeError('candidate changed before startup activation')
        result=agent(['--headed','focus-window','--window-id',str(window['id'])])
        actions.append({'utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'action':'owned-startup-window-focus','pid':identity.pid,'birth':list(identity.birth),'window_id':window['id'],'result':result})
        if not result.get('ok'):raise RuntimeError('owned startup activation rejected')
       else:
        actions.append({'utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'action':'owned-startup-window-already-focused','pid':identity.pid,'birth':list(identity.birth),'window_id':window['id']})
       focused.add(key);state['last_activated_pid']=identity.pid;save()
   partial=out/'observations.partial.json'
   if partial.exists():
    observed=json.loads(partial.read_text())
    state['native_assertions_observed']=len(observed.get('checks',[]))
   files=[p for p in out.glob('*.json') if p.name not in ['vm-operator-actions.json','candidate-processes.json']]
   if files:
    state['latest_evidence']=max(files,key=lambda p:p.stat().st_mtime).name
   phase=(state.get('last_activated_pid'),state['native_assertions_observed'],state.get('latest_evidence'))
   if phase!=last_report:
    print('VM native progress',phase,flush=True);last_report=phase
   save()
  except (json.JSONDecodeError,FileNotFoundError):
   pass
  except Exception as e:
   actions.append({'utc':time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()),'operator_error':type(e).__name__+': '+str(e)});save()
  time.sleep(.5)
state.update(state='finished',exit_code=proc.returncode,finished_utc=time.strftime('%Y-%m-%dT%H:%M:%SZ',time.gmtime()))
if (out/'observations.json').exists():
 raw=json.loads((out/'observations.json').read_text())
 state['native_assertions_observed']=len(raw.get('checks',[]));state['errors']=raw.get('errors',[])
save()
print('VM native finished',json.dumps(state),flush=True)
sys.exit(proc.returncode)
