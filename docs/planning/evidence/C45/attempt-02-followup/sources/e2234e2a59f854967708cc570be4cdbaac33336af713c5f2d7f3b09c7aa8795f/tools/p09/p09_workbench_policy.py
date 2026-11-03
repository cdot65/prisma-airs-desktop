"""G11: explicit P09 Workbench lifecycle transition; historical reviews immutable.

Current application bytes require current20 native/current18 checks. Closed C31,
C32 subjects retain actual all9; the cumulative phase inspects all new code.
"""
from pathlib import Path
import hashlib,json,subprocess
import phase_inputs as original
import c32_production_policy as prior
from c32_production_policy import OWNER_SCORE,OWNER_SHA,bundle_digest,historical_c29,historical_performance
PREFIX='docs/planning/evidence/P09/remediation-attempt-01/'
ARCHIVE='docs/planning/evidence/P09/native-history/Worf-generation-10-complete-before-P09-workbench-lifetime-20261102/'
G9=prior.ARCHIVE
BASE='docs/planning/reviews/P09/attempt-01/'
BASE_COMMIT='4dc775decc53d069a0854858493b284a0704dfd7'
BASE_DIGEST='2ced3ba55ff87ea3652028dc3c25937375322eaafef4b6956e1e3133afeca265'
FIX_PATHS={'crates/desktop-core/src/authority.rs','crates/desktop-core/src/authority/workbench_domain.rs','crates/desktop-core/src/authority/workbench_domain/send.rs','crates/desktop-core/tests/workbench_execution/main.rs','apps/desktop/src-tauri/Cargo.toml','apps/desktop/src-tauri/tests/workbench_review_lifecycle.rs'}
ADMIN_PATHS={'.gitattributes',*('tools/p09/'+n for n in ['p09_workbench_policy.py','test_p09_workbench_policy.py','phase_inputs_p09_workbench.py','phase_packet_p09_workbench.py','make-phase-packet-p09-workbench.py','audit-phase-review-p09-workbench.py','audit-p09-checks-workbench.py'])}
def digest(x):return hashlib.sha256((json.dumps(x,sort_keys=True,separators=(',',':'))+'\n').encode()).hexdigest()
def workbench_projection(text):
    line='        state.workbench.sending.retire_renderer_review();\n'
    original.require(text.count(line)==1,'Workbench retirement boundary ambiguous')
    return text.replace(line,'')
def committed(root,commit,name):
    raw=subprocess.check_output(['git','show',commit+':'+name],cwd=root)
    original.require(hashlib.sha256(raw).hexdigest()==original.sha(root/name),'frozen raw record changed: '+name)
def verify_transition(root):
    from phase_packet_owner_closure import source_inventory
    root=Path(root);proof=original.read(root/PREFIX/'transition.json')
    for name in ['source-inventory.json','request.json','verdict-returned.json','post-review-audit.json']:
        committed(root,BASE_COMMIT,BASE+name)
    old=original.read(root/BASE/'source-inventory.json')['files']
    original.require(digest(old)==BASE_DIGEST,'frozen P09 source digest changed')
    now=source_inventory(root)
    delta={n:{'before':old.get(n),'after':now.get(n)} for n in sorted(set(old)|set(now)) if old.get(n)!=now.get(n)}
    original.require(delta==proof['source_delta'] and set(delta)<=FIX_PATHS|ADMIN_PATHS,'undeclared P09 Workbench production/admin delta')
    original.require(set(delta)>=FIX_PATHS and digest(now)==proof['current_source_digest'],'incomplete or stale source transition')
    original.require(workbench_projection((root/'crates/desktop-core/src/authority.rs').read_text())==(root/ARCHIVE/'reviewed-inputs/crates/desktop-core/src/authority.rs').read_text(),'shared authority changed outside Workbench page retirement')
    for item in proof['focused_results']:
        record=original.read(root/item['record'])
        original.require(record['exit']==0 and original.sha(root/item['record'])==item['sha256'] and original.sha(root/record['log'])==record['log_sha256'],'focused preflight drift')
    original.require(bool(proof['focused_results']),'missing affected preflight')
    original.require(subprocess.run(['git','merge-base','--is-ancestor',proof['fix_commit'],'HEAD'],cwd=root,capture_output=True).returncode==0,'Workbench fix not integrated')
    names=set(subprocess.check_output(['git','diff-tree','--no-commit-id','--name-only','-r',proof['fix_commit']],cwd=root,text=True).splitlines())
    original.require(names==FIX_PATHS,'fix commit contains unrelated changes')
    original.require(prior.authority_projection((root/G9/'reviewed-source/authority.rs').read_text())==prior.authority_projection(workbench_projection((root/'crates/desktop-core/src/authority.rs').read_text())),'closed C31 shared branches changed')
    return {'record':PREFIX+'transition.json','sha256':original.sha(root/PREFIX/'transition.json'),'current_source_digest':digest(now),'changed_paths':sorted(delta),'standing':'New P09 Workbench page-owned review production delta independently inspected by cumulative P09. Actual C31attempt07/C32attempt23 all9 remain historical on unchanged subjects, with exact original inputs archived; no scope-hash equivalence or score substitution. New G11 application requires actual current20 native/current18 checks.'}
def historical_member(root,subject,attempt,commit):
    root=Path(root);verify_transition(root);out=root/'docs/planning/reviews'/subject/attempt
    v=original.read(out/'verdict-returned.json');r=original.read(out/'request.json');a=original.read(out/'post-review-audit.json');receipt=original.read(out/'receipt.json')
    original.full_gate(v)
    for name in ['request.json','verdict-returned.json','post-review-audit.json','receipt.json','source-inventory.json']:committed(root,commit,str((out/name).relative_to(root)))
    from phase_inputs_c32_production import verify_launch
    original.require(verify_launch(root,out)==a['transport'],'original independent transport drift')
    original.require(v['request_id']==r['request_id'] and v['fingerprints']==r['fingerprints'],'original request binding')
    original.require(a['full_gate_pass'] is True and a['raw_verdict_sha256']==original.sha(out/'verdict-returned.json') and a['request_sha256']==original.sha(out/'request.json'),'actual all9 audit binding')
    original.require(receipt['full_gate_pass'] is True and receipt['verdict_sha256']==original.sha(out/'verdict-returned.json') and receipt['post_review_audit_sha256']==original.sha(out/'post-review-audit.json'),'actual all9 receipt binding')
    for name,h in original.read(out/'source-inventory.json')['files'].items():
        choices=[root/name,root/ARCHIVE/'reviewed-inputs'/name]
        if name.startswith('docs/planning/evidence/P09/'):
            rel=name.removeprefix('docs/planning/evidence/P09/');choices += [root/ARCHIVE/rel,root/G9/rel]
        if name in ['crates/desktop-core/src/authority.rs','tools/native-acceptance/legacy_assertions.py','.gitattributes']:
            choices.append(root/G9/'reviewed-source'/Path(name).name)
        original.require(any(p.is_file() and original.sha(p)==h for p in choices),'exact historical scope not retained: '+subject+' '+name)
    result={'subject':subject,'scores':v['scores'],'recommendation':v['recommendation'],'standing':'Actual '+attempt+' all9 retained on unchanged '+subject+' subject; exact former broad inputs archived, not claimed current. Current Workbench-only delta and fresh G11 qualification reviewed cumulatively by P09. No owner activation or re-review.'}
    for key,name in [('record','verdict-returned.json'),('request','request.json'),('audit','post-review-audit.json'),('receipt','receipt.json')]:
        result[key]=str((out/name).relative_to(root));result['sha256' if key=='record' else key+'_sha256']=original.sha(out/name)
    return result
def historical_c31(root):return historical_member(root,'C31','attempt-07','3aaf67ad386fa3817d180ff76ec2d2f76338dd30')
def historical_c32(root):return historical_member(root,'C32','attempt-23','bb990bdd')

def verify_c30_owner_closure(root):
    root=Path(root);baseline=prior.baseline
    for name,h in baseline.PINNED.items():original.require(original.sha(root/name)==h,'historical C30 record changed: '+name)
    latest,v=original.latest_verdict(root,'C30');original.require(latest==root/baseline.ATTEMPT/'verdict-returned.json' and v['scores']==baseline.SCORES and v['recommendation']=='fail','C30 raw historical result changed')
    original.require(original.sha(root/OWNER_SCORE)==OWNER_SHA,'C30 owner decision changed')
    result={'subject':'C30','owner_closed':True,'scores':v['scores'],'recommendation':'fail','full_gate_pass':False,'owner_scores':{k:9 for k in baseline.SCORES},'owner_decision':OWNER_SCORE,'owner_decision_sha256':OWNER_SHA,'transition':verify_transition(root),'standing':'Owner all9 C30 standing continues across unrelated declared P09 Workbench delta; actual historical fail/auditfalse unchanged. C30 is closed and cannot lower P09; no current C30 qualification or independent score claimed.'}
    for key,name in [('record','verdict-returned.json'),('request','request.json'),('receipt','receipt.json'),('audit','post-review-audit.json')]:
        result[key]=baseline.ATTEMPT+'/'+name;result['sha256' if key=='record' else key+'_sha256']=original.sha(root/result[key])
    return result
