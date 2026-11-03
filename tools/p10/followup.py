"""Explicit affected C45 attempt-01 followup, never a relabeled baseline.

The original 142-ID qualification and actual 9/9/9/8/8 review remain immutable.
Only named synthetic tests and administrative proof consumers may differ. The
current source, original exact app, unchanged runtime helpers and real focused
Runner results are all required before a fresh independent review.
"""
from pathlib import Path
import json,platform
from qualification import read,sha,require,bound,verify as verify_coverage
from candidate import source_inventory,digest,verify_candidate,verify_build_reuse

ALLOWED={
 'crates/xtask/tests/controlled_live.rs','crates/xtask/tests/live_run/common.rs',
 'crates/xtask/examples/c45_baseline.rs','tools/p10/candidate.py',
 'tools/p10/packet.py','tools/p10/followup.py','tools/p10/test_followup.py',
}
COMMANDS=[
 ('C45/followup-live',['cargo','test','--locked','-p','xtask','--features','test-fixtures','--test','controlled_live','--test','live_run'],True),
 ('C45/followup-live-clippy',['cargo','clippy','--locked','-p','xtask','--features','test-fixtures','--test','controlled_live','--test','live_run','--example','c45_baseline','--','-D','warnings'],False),
 ('C45/followup-fmt',['cargo','fmt','--all','--','--check'],False),
]
META_ONLY={'tools/p10/candidate.py'}


def source_delta(before,after,declared):
    changed=sorted(n for n in set(before)|set(after) if before.get(n)!=after.get(n))
    require(changed==declared and set(changed)<=ALLOWED,'undeclared or application-affecting followup delta')
    return changed


def raw_checks(root,record,inputs):
    raw=read(bound(root,record))
    require(raw['schema']=='airs-v2/run/1' and raw['subject']=='C45' and raw['status']=='passed' and raw['failure'] is None,'affected runner did not pass')
    require(raw['inputs_before']==raw['inputs_after']==inputs,'affected runner source/dependency/flags drift')
    require(len(raw['commands'])==len(COMMANDS),'affected command membership')
    for command,(label,argv,tests) in zip(raw['commands'],COMMANDS):
        require(command['label']==label and command['argv']==argv and command['cwd'] in ['.',str(root)] and command['status']=='passed' and command['exit_code']==0 and not command['note'],'affected command mismatch/failure')
        require(sha(root/record['path']).strip()==record['sha256'],'raw runner changed')
        log=(root/record['path']).parent/command['log']
        require(log.resolve().is_relative_to(root.resolve()) and sha(log)==command['output_sha256'],'affected log drift')
        if tests:require(command['tests'] and command['tests']['passed']>0 and command['tests']['failed']==0,'affected tests not actually executed')
    return {'run_id':raw['run_id'],'commands':len(COMMANDS),'tests':raw['commands'][0]['tests']}


def verify(root,record,current=True):
    require(record['schema']=='airs-desktop/c45-affected-followup/1','followup schema')
    baseline=read(bound(root,record['baseline_qualification']));base_result=verify_coverage(root,baseline)
    old=read(bound(root,baseline['candidate']));candidate=read(bound(root,record['candidate']))
    before=read(bound(root,record['source_before']))['files'];after=read(bound(root,record['source_after']))['files']
    require(digest(before)==baseline['source_inventory_sha256']==old['source_inventory_sha256'],'baseline source binding')
    source_delta(before,after,record['declared_changed_paths'])
    require(digest(after)==candidate['source_inventory_sha256']==record['source_inventory_sha256'],'current followup source binding')
    if current:require(source_inventory(root)==after,'followup current source drift')
    # No historical source/helper hash is relabeled as current. The entire
    # application/config/dependency tree outside the named delta is unchanged.
    for key in ('executable_sha256','bundle_sha256','build_flags','conditions','dependency_fingerprint'):
        require(candidate[key]==old[key],'followup app/build/conditions/dependencies changed: '+key)
    require(set(candidate['helper_closure'])==set(old['helper_closure']),'runtime helper membership changed')
    for name,value in old['helper_closure'].items():
        if name not in META_ONLY:require(candidate['helper_closure'][name]==value==sha(root/name),'runtime helper changed: '+name)
    verify_candidate(root,candidate)
    checks=raw_checks(root,record['focused_run'],candidate['runner_inputs'])
    for failure in record['old_red']:
        path=bound(root,failure);require(failure['exit_code']==101 and 'test result: FAILED.' in path.read_text(),'causal original failure proof missing')
    review=root/'docs/planning/reviews/C45/attempt-01'
    for name,ref in record['actual_previous_review'].items():require(bound(root,ref)==review/name,'previous actual review binding')
    audit=read(review/'post-review-audit.json');require(audit['scores']=={'architecture':9,'behavior':9,'authority_security':9,'validation':8,'overall':8} and audit['full_gate_pass'] is False,'actual previous review changed')
    return {'qualified':True,'required':base_result['required']+len(COMMANDS),'baseline':base_result,'affected':checks,'source_inventory_sha256':digest(after),'candidate_sha256':record['candidate']['sha256'],'meaning':'Original baseline stays historical; named test/provenance-only delta, exact unchanged app/runtime helpers and new affected checks verified. No new native or baseline execution claimed.'}


def prepare(root):
    import copy
    folder=root/'docs/planning/evidence/C45/attempt-01-followup';require(not folder.exists(),'preserve prior followup');folder.mkdir()
    ref=lambda p:dict(path=str(p.relative_to(root)),sha256=sha(p))
    baseline=root/'docs/planning/evidence/C45/qualified-coverage.json';old=read(bound(root,read(baseline)['candidate']))
    previous=root/'docs/planning/evidence/C45/baseline-03/continuation-04/source-seal.json'
    before=read(previous)['files'];after=source_inventory(root);changed=sorted(n for n in set(before)|set(after) if before.get(n)!=after.get(n));source_delta(before,after,changed)
    seal=folder/'source-seal.json';seal.write_text(json.dumps(dict(files=after,source_fingerprint=digest(after)),indent=2)+'\n')
    proof=copy.deepcopy(read(root/old['build_origin']['record']));proof['current_source']=ref(seal);origin=read(bound(root,proof['original_source']))['files'];proof['declared_changed_paths']=sorted(n for n in set(origin)|set(after) if origin.get(n)!=after.get(n));proof['reason']='Actual C45attempt01 synthetic fixture namespace remediation plus focused Runner/proof consumers only; exact application/config/dependencies/native helpers unchanged. No rebuild.';proof['checks_to_execute']='Only affected synthetic controlled_live/live_run tests, their Clippy and formatting. Original142 baseline results retained unchanged.'
    proof_path=folder/'build-reuse.json';proof_path.write_text(json.dumps(proof,indent=2)+'\n')
    candidate=copy.deepcopy(old);candidate.update(source_inventory_sha256=digest(after),helper_closure={n:sha(root/n) for n in old['helper_closure']})
    # Filled from the actual current cheap Runner identity before focused tests.
    return folder,candidate,proof


if __name__=='__main__':
    import sys
    root=Path.cwd().resolve()
    if sys.argv[1:]==['verify']:
        p=root/'docs/planning/evidence/C45/attempt-01-followup/qualified-coverage.json';print(json.dumps(verify(root,read(p)),indent=2))
