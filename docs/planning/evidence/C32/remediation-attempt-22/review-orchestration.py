"""Fresh C31/C32/P09 actual reviews after completed G9; final C31 owner cap explicit."""
from pathlib import Path

import datetime,fcntl,hashlib,json,os,re,subprocess,sys,time,traceback
ROOT=Path('/Users/cdot/development/redtail/prisma-airs-desktop-v2-current');os.chdir(ROOT)
RUN=ROOT/'.evidence-private/worf-agent/2026-11-02-c32-production-01'
OUT=RUN/'signoffs';OUT.mkdir(exist_ok=True)
BASE=Path('/Users/admin/.agent-workflows/airs/2026-11-01-worf-start')
ENV=os.environ.copy();ENV['PATH']=str(ROOT/'.tools/bin')+':/Users/admin/.cargo/bin:/Users/admin/.nvm/versions/node/v24.21.0/bin:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin'
ORDER=['architecture','behavior','authority_security','validation','overall']
def read(p):return json.loads(Path(p).read_text())
def sha(p):return hashlib.sha256(Path(p).read_bytes()).hexdigest()
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def status(state,**fields):
 v={'time':now(),'signoff_supervisor_pid':os.getpid(),'state':state,**fields}
 with (OUT/'events.jsonl').open('a') as f:f.write(json.dumps(v)+'\n')
 (RUN/'runstatus.md').write_text(json.dumps(v,indent=2)+'\n')
 p=BASE/'active-agent.json';x=read(p);x.update(time=v['time'],state=state,signoff_supervisor_pid=os.getpid(),active_review_subject=fields.get('subject'),active_review_attempt=fields.get('attempt'));p.write_text(json.dumps(x,indent=2)+'\n')
 print(json.dumps(v),flush=True)
def command(cmd,label):
 with (OUT/(label+'.log')).open('x') as f:r=subprocess.run(cmd,cwd=ROOT,env=ENV,stdin=subprocess.DEVNULL,stdout=f,stderr=subprocess.STDOUT)
 assert r.returncode==0,label+' failed'
def review(subject,attempt,prior=None):
 assert subject in ('C32','P09'), 'C31 review is closed at actual all9'
 p=ROOT/'docs/planning/reviews'/subject/attempt
 status('Preparing fresh '+subject+' '+attempt,subject=subject,attempt=attempt)
 if subject=='P09':
  command([sys.executable,'-I','tools/p09/make-phase-packet-c32-production.py',attempt,'docs/planning/evidence/P09/runs/run-attempt-10.json','C30 owner all9 controls; its raw historical low verdict must not lower P09; unchanged closed findings are not phase remediation. Inspect the entire current C32 Harness delta and administrative transition; C31 actualall9 stays closed.'],'prepare-'+subject+'-'+attempt)
 else:command([sys.executable,'-I','tools/p09/make-c32-production-packet.py',subject,attempt,prior,'attempt-10'],'prepare-'+subject+'-'+attempt)
 if subject != 'P09':
  qualified = inputs.verify_native(ROOT)
  request = read(p/'request.json')
  request['native_acceptance']['build_input_qualification'] = qualified['build_inputs']
  request['source_transition'] = policy.verify_transition(ROOT)
  request['limitations'].append('Current G9 freshly qualifies the final attempt06 scoped native ownership and successful Gateway-detail fixture/checker fix. G8 all19/all18 and actual accepted9/8/8/8/8 remain historical and immutable, with their exact application archived. No G8 application/native pass is claimed as current. C31 now has one final actual review; any score below9 activates its bound ownerall9 completion regardless of blockers, without changing raw results or authorizing further C31 cycles. C32/P09 actual independent full9 remain required.')
  (p/'request.json').write_text(json.dumps(request,indent=2)+'\n')
  with (p/'message.txt').open('a') as f:f.write('\nInspect the current scoped presentation and successful-detail fix, original old-red/new-green evidence and freshly qualified G9 inputs. Honor the explicit final C31 review cap; raw owner standing and independent scores must remain separate.\n')
 status('Actual fresh independent read-only gpt-6.1-sol review running',subject=subject,attempt=attempt)
 command([sys.executable,'tools/p09/launch-remedial-review.py',subject,attempt,'gpt-6.1-sol'],'launch-'+subject+'-'+attempt)
 v=read(p/'verdict-returned.json')
 status('ACTUAL VERDICT '+subject+' '+attempt+' '+ '/'.join(str(v['scores'][k]) for k in ORDER),subject=subject,attempt=attempt,scores=v['scores'],recommendation=v['recommendation'],required_fixes=v['required_fixes'],blockers=v['blockers'],evidence_gaps=v['evidence_gaps'])
 audit='tools/p09/audit-phase-review-c32-production.py' if subject=='P09' else 'tools/p09/audit-c32-production-review.py'
 command([sys.executable,'-I',audit,*([attempt] if subject=='P09' else [subject,attempt])],'audit-'+subject+'-'+attempt)
 a=read(p/'post-review-audit.json');r=read(p/'request.json')
 full=a['full_gate_pass'];eligible=subject!='P09' and min(v['scores'].values())>=8 and not v['blockers']
 owner_trigger = subject == 'C31' and any(n < 9 for n in v['scores'].values())
 if full or eligible or owner_trigger:
  x={'review_id':r['request_id'],'subject':subject,'attempt':attempt,'request_id':r['request_id'],'transport':r['transport'],'time':now(),'model_requested':'gpt-6.1-sol','harness_version_observed':a['transport']['harness_version_observed'],'reviewer_sessions_observed':a['transport']['reviewer_sessions_observed'],'scores':v['scores'],'recommendation':v['recommendation'],'required_fixes':v['required_fixes'],'blockers':v['blockers'],'evidence_gaps':v['evidence_gaps'],'optional_suggestions':v['optional_suggestions'],'fingerprints':v['fingerprints'],'verdict_sha256':sha(p/'verdict-returned.json'),'request_sha256':sha(p/'request.json'),'prompt_sha256':sha(p/'message.txt'),'post_review_audit_sha256':sha(p/'post-review-audit.json'),'invocation_sha256':sha(p/'invocation.json'),'thread_reply_sha256':sha(p/'thread-reply.log'),'source_frozen':True,'d034_accepted':eligible and not full,'full_gate_pass':full,'controller_complete':False,'limitations':v['limitations']+['Fresh separate configured-model read-only session in implementer model family; hash consistency is not cryptographic provider/compiler authorship. C26/P08 controller/release remain open.'],'outcome':'Actual independent current G10 review audited. Historical G7/C29 evidence and C30/C41 owner decisions remain explicitly distinguished.'}
  if subject=='P09':x.update(owner_c30_scores={k:9 for k in ORDER},owner_c30_score_record='docs/planning/C30-owner-score-override-2026-11-02.md',owner_c30_score_record_sha256='c150ebb6a35d8b1bdb6965a5780b96b88f393ed2994cdd3a11da2b4d20bd1a5e')
  with (p/'receipt.json').open('x') as f:json.dump(x,f,indent=2);f.write('\n')
  (p/'acceptance.md').write_text('Actual '+subject+' '+attempt+' scores '+ '/'.join(str(v['scores'][k]) for k in ORDER)+('. Independent full gate passed.' if full else ('. Final C31 raw result retained; conditional owner completion is recorded separately; no further C31 cycles authorized.' if owner_trigger else '. Accepted under D034 with findings retained; independent full-nine standing remains required for this subject.'))+'\n\nC30 stands at owner all9; independent raw low verdict unchanged. No C30 work, current performance claim, live call, controller completion or release is inferred.\n')
 owner_completion = None
 if owner_trigger:
  from c31_owner_completion import activate
  owner_completion = activate(ROOT)
  status('C31 OWNER ALL9 ACTIVATED; raw actual scores/findings/blockers/gaps and false full-gate audit preserved; no further C31 cycles authorized',subject=subject,attempt=attempt,actual_scores=v['scores'],owner_scores=owner_completion['owner_scores'],activation=owner_completion['activation'])
 command(['git','add','--',str(p.relative_to(ROOT)),*([owner_completion['activation']] if owner_completion else [])],'stage-'+subject+'-'+attempt)
 command(['git','commit','-m',('Record actual '+subject+' full-nine current review' if full else ('Record actual final C31 review and conditional owner completion' if owner_trigger else 'Retain actual '+subject+' review and accepted findings'))],'commit-'+subject+'-'+attempt)
 assert full or owner_completion,'Actual '+subject+' review has no authorized full-nine completion' 
 status(subject+(' owner all-nine completion validated; raw independent score unchanged' if owner_completion else ' actual full-nine passed and audited'),subject=subject,attempt=attempt,scores=v['scores'],owner_completion=owner_completion)
lock=(OUT/'supervisor.lock').open('a+');fcntl.flock(lock,fcntl.LOCK_EX|fcntl.LOCK_NB)
try:
 sys.path.insert(0,str(ROOT/'tools/p09'))
 import c32_production_policy as policy
 import phase_inputs_c32_production as inputs
 policy.verify_transition(ROOT);inputs.verify_native(ROOT)
 inputs.verify_checks(ROOT,ROOT/'docs/planning/evidence/P09/runs/run-attempt-10.json')
 assert (RUN/'qualification-complete.json').is_file(),'Actual completed G10 required'
 paths=['docs/planning/evidence/P09/native','docs/planning/evidence/P09/parity','docs/planning/evidence/P09/screens','docs/planning/evidence/P09/native-prepare.json','docs/planning/evidence/P09/native-build-inputs.json','docs/planning/evidence/P09/native-acceptance.json','docs/planning/evidence/P09/execution.json','docs/planning/evidence/P09/native-history/Worf-generation-09-complete-before-C32-scoped-confirmation-20261102','docs/planning/evidence/P09/preparations/worf-10-c32-scoped-confirmation','docs/planning/evidence/P09/runs/attempt-10','docs/planning/evidence/P09/runs/run-attempt-10.json']
 command(['git','add','--',*paths],'stage-current-qualification')
 command(['git','commit','-m','Retain actual G10 current C32 native20 and repository18 qualification'],'commit-current-qualification')
 review('C32','attempt-23','attempt-22')
 phase='attempt-01'
 while (ROOT/'docs/planning/reviews/P09'/phase).exists():phase='attempt-'+str(int(phase.split('-')[1])+1).zfill(2)
 review('P09',phase)
 status('P09 DONE: actual independent phase full-nine passed and audited; C45/P10 now authorized',phase_receipt='docs/planning/reviews/P09/'+phase+'/receipt.json')
 (OUT/'p09-done.json').write_text(json.dumps({'time':now(),'phase_attempt':phase,'receipt':'docs/planning/reviews/P09/'+phase+'/receipt.json'},indent=2)+'\n')
 command([sys.executable,'-I',str(RUN/'verify-c45-activation.py'),str(ROOT)],'c45-activation-proof')
except BaseException as error:
 status('Signoffs stopped on concrete actual condition; no automatic tests/review retries',error=str(error),traceback=traceback.format_exc())
 raise
