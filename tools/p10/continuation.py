"""Explicit test-only continuation; preserves passed raw stages and their inputs.

Only the stale C45 permission-manifest test and named provenance consumers may
change. The successful app build and completed checks are never relabeled as
new executions. The failed Host aggregate is executed again to finish its
previously unrun targets; unrelated Core/frontend/native work is not restarted.
"""
from pathlib import Path
import copy,json,shutil
from qualification import read,sha,require
from candidate import source_inventory,digest,helper_closure
from stages import verify_completed

ALLOWED={
 'apps/desktop/src-tauri/tests/permission_manifest.rs',
 'tools/p10/candidate.py','tools/p10/supervise.py','tools/p10/finalize.py',
 'tools/p10/continuation.py','tools/p10/test_continuation.py',
}


def paths(base):
    marker=base/'continuation-01.json'
    folder=base/'continuation-01' if marker.exists() else base
    return dict(folder=folder,source=folder/'source-seal.json',candidate=folder/'candidate.json',plan=folder/'required-coverage.json',stages=folder/'stages',reuse=folder/'build-reuse.json')


def verify(root,base):
    marker=read(base/'continuation-01.json');current=paths(base)
    require(marker['schema']=='airs-desktop/c45-test-only-continuation/1' and marker['affected_stage']=='command-C45-host-2','continuation schema/scope')
    before=read(base/'source-seal.json')['files'];after=read(current['source'])['files']
    require(sha(current['source'])==marker['source_seal_sha256'],'continuation seal changed')
    changed=sorted(name for name in set(before)|set(after) if before.get(name)!=after.get(name))
    require(changed==marker['declared_changed_paths'] and set(changed)<=ALLOWED,'undeclared/application source continuation delta')
    require(source_inventory(root)==after,'continuation source membership/content changed')
    require(sha(base/'candidate.json')==marker['original_candidate_sha256'],'original candidate changed')
    origin=read(base/'candidate.json')
    for stage in marker['retained_completed_stages']:
        require(stage!='command-C45-host-2','failed/affected Host stage cannot be retained')
        folder=base/'stages'/stage;state=read(folder/'completed.json')
        verify_completed(root,folder,state,state['argv'],digest(before),sha(base/'candidate.json'))
    for check in marker['focused_checks']:
        log=root/check['log'];require(sha(log)==check['log_sha256'] and check['exit_code']==0,'affected focused check failed/drifted')
        require(check['source_sha256']==sha(root/'apps/desktop/src-tauri/tests/permission_manifest.rs'),'focused check source drift')
    return marker


def retained(root,base,stage,argv):
    if not (base/'continuation-01.json').exists():return None
    marker=verify(root,base)
    if stage not in marker['retained_completed_stages']:return None
    folder=base/'stages'/stage;state=read(folder/'completed.json')
    require(state['argv']==argv,'retained command differs')
    return folder,state,base/'candidate.json'


def prepare(root):
    base=root/'docs/planning/evidence/C45/baseline-03';folder=base/'continuation-01'
    require(not folder.exists() and not (base/'continuation-01.json').exists(),'preserve prior continuation')
    folder.mkdir();source=source_inventory(root);before=read(base/'source-seal.json')['files']
    changed=sorted(n for n in set(before)|set(source) if before.get(n)!=source.get(n))
    require(set(changed)<=ALLOWED,'application-affecting continuation not authorized by this policy')
    old=read(base/'candidate.json');plan=copy.deepcopy(read(base/'required-coverage.json'))
    completed=[]
    for file in sorted((base/'stages').glob('*/completed.json')):
        if file.parent.name=='command-C45-host-2':continue
        state=read(file);verify_completed(root,file.parent,state,state['argv'],digest(before),sha(base/'candidate.json'));completed.append(file.parent.name)
    for row in plan['requirements']:
        if row['kind']=='command' and 'command-'+row['label'].replace('/','-') in completed:
            row.update(must_execute=False,affected=False,continuation='Original passing command retained on exact app/flags/conditions; only test identity assertions and administrative proof consumers changed. Current focused permission test/lint cover the changed target. No new execution or old-input fingerprint equivalence claimed.')
    (folder/'source-seal.json').write_text(json.dumps({'files':source,'source_fingerprint':digest(source)},indent=2)+'\n')
    plan['helper_closure']={name:sha(root/name) for name in plan['helper_closure']}
    (folder/'required-coverage.json').write_text(json.dumps(plan,indent=2)+'\n')
    focused=[];private=root/'.evidence-private/worf-agent/2026-11-02-c45-p10-01'
    for name,argv in [('permission-manifest-current-01.log',['cargo','test','--locked','-p','desktop-host','--features','test-fixtures','--test','permission_manifest']),('permission-manifest-clippy-01.log',['cargo','clippy','--locked','-p','desktop-host','--features','test-fixtures','--test','permission_manifest','--','-D','warnings'])]:
        target=folder/name;shutil.copy2(private/name,target)
        text=target.read_text();require(('6 passed; 0 failed' in text) if 'current' in name else ('Finished `dev` profile' in text),'focused check output not passing')
        focused.append(dict(argv=argv,exit_code=0,log=str(target.relative_to(root)),log_sha256=sha(target),source_sha256=sha(root/'apps/desktop/src-tauri/tests/permission_manifest.rs')))
    marker=dict(schema='airs-desktop/c45-test-only-continuation/1',affected_stage='command-C45-host-2',original_candidate_sha256=sha(base/'candidate.json'),source_seal_sha256=sha(folder/'source-seal.json'),declared_changed_paths=changed,retained_completed_stages=completed,focused_checks=focused,reason='Real Host failure was a test still enforcing retired v2 IDs. Strengthened current1.0 names/version/old-ID exclusion; no application/config/dependency or native driver/helper behavior changed. Preserve actual failed Host record and all13 successful commands/current smoke. Execute affected Host aggregate and first remaining checks/native queue; no rebuild or unrelated repeat.')
    (base/'continuation-01.json').write_text(json.dumps(marker,indent=2)+'\n')
    verify(root,base);return marker


if __name__=='__main__':print(json.dumps(prepare(Path.cwd().resolve()),indent=2))
