"""Metadata-only archival correction after real C45 attempt-02.

Retains original145 qualification, source/execution/condition bindings and raw
scores. No application/native/Rust execution is implied by these Python checks.
"""
import re
from qualification import read,sha,require,bound
from candidate import source_inventory,digest,verify_candidate
from historical import Historical

ALLOWED={'tools/p10/candidate.py','tools/p10/qualification.py','tools/p10/followup.py',
 'tools/p10/packet.py','tools/p10/review_gate.py','tools/p10/historical.py',
 'tools/p10/provenance_followup.py','tools/p10/test_historical.py'}
TESTS=['test_historical.py','test_followup.py','test_build_reuse.py','test_qualification.py','test_review_gate.py']


def verify(root,record,current=True,historical=None):
    require(record['schema']=='airs-desktop/c45-provenance-followup/1','provenance followup schema')
    require(current or historical is not None,'historical qualification needs explicit archive context')
    context=Historical(root,record['historical_context'])
    if historical:require(historical.reference==context.reference,'historical context binding differs')
    from packet import qualification
    previous=qualification(root,bound(root,record['parent_qualification']),current=False,historical=context)
    candidate=read(bound(root,record['candidate']));parent=read(bound(root,read(bound(root,record['parent_qualification']))['candidate']))
    before=read(bound(root,record['source_before']))['files'];after=read(bound(root,record['source_after']))['files']
    require(digest(before)==parent['source_inventory_sha256'],'previous source binding')
    changed=sorted(n for n in set(before)|set(after) if before.get(n)!=after.get(n))
    require(changed==record['declared_changed_paths'] and set(changed)<=ALLOWED,'undeclared/application provenance delta')
    require(digest(after)==candidate['source_inventory_sha256']==record['source_inventory_sha256'],'current provenance source binding')
    if current:require(source_inventory(root)==after,'current provenance source drift')
    else:context.source(digest(after))
    for key in ('executable_sha256','bundle_sha256','build_flags','conditions','dependency_fingerprint'):
        require(candidate[key]==parent[key],'provenance app/dependencies/conditions changed')
    require(set(candidate['helper_closure'])==set(parent['helper_closure']),'provenance helper membership')
    for name,value in parent['helper_closure'].items():
        if name!='tools/p10/candidate.py':require(candidate['helper_closure'][name]==value,'runtime helper changed')
    verify_candidate(root,candidate,historical=None if current else context)
    checks=read(bound(root,record['focused_checks']));require(checks['schema']=='airs-desktop/focused-python-checks/1' and checks['source_before']==checks['source_after']==digest(after),'focused Python source drift')
    require(len(checks['commands'])==len(TESTS),'focused Python target membership')
    for command,name in zip(checks['commands'],TESTS):
        argv=['/usr/bin/python3','-B','-m','unittest','discover','-s','tools/p10','-p',name,'-v']
        require(command['argv']==argv and command['exit_code']==0 and command['cwd']==candidate['execution_root'] and command['started_utc'] and command['finished_utc'],'focused Python actual command binding')
        output=bound(root,command['log']).read_text();require(re.search(r'Ran [1-9][0-9]* tests',output) and '\nOK\n' in output,'focused Python tests not passing')
    for name,ref in record['actual_previous_review'].items():require(bound(root,ref)==root/'docs/planning/reviews/C45/attempt-02'/name,'previous actual review binding')
    return {'qualified':True,'required':previous['verified']['required']+len(TESTS),'previous':previous,'focused_python_targets':len(TESTS),'source_inventory_sha256':digest(after),'candidate_sha256':record['candidate']['sha256'],'meaning':'Historical archives validate original sources/application/helpers/conditions independently of current checkout or builder. Current reuse remains strict. Original145coverage and72synthetic tests retained; only affected Python proof tests executed.'}
