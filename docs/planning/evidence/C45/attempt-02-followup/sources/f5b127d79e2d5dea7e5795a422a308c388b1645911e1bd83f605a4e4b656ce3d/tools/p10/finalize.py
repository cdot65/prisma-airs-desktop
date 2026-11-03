"""Normalize actual original execution bindings, then strictly consume them."""
from pathlib import Path
import json,sys
from qualification import read,sha,require,verify
from candidate import verify_candidate,verify_source
from seal import verify_plan
from stages import verify_completed
import continuation


def ref(root,path):return {'path':str(path.relative_to(root)),'sha256':sha(path)}


def finish(root):
    base=root/'docs/planning/evidence/C45/baseline-03';layout=continuation.paths(base);plan=read(layout['plan'])
    verify_plan(root,plan)
    marker=continuation.verify(root,base) if (base/'continuation-01.json').exists() else None
    source=verify_source(root,str(layout['source'].relative_to(root)))
    candidate=read(layout['candidate']);verify_candidate(root,candidate)
    require(candidate['source_inventory_sha256']==source,'candidate source drift')
    candidate_ref=ref(root,layout['candidate']);rows=[]
    for needed in plan['requirements']:
        cid=needed['id']
        stage='command-'+needed['label'].replace('/','-') if needed['kind']=='command' else 'native-'+cid.split('-')[0].lower()
        state_folder=layout['stages']/stage
        reused=not (state_folder/'completed.json').exists()
        if reused:
            require(marker and stage in marker['retained_completed_stages'],'stage is not explicitly eligible for reuse')
            state_folder=base/'stages'/stage
        state=read(state_folder/'completed.json')
        origin_ref=ref(root,base/'candidate.json') if reused else candidate_ref
        origin=read(root/origin_ref['path'])
        kind='check' if needed.get('label','').startswith('common/') else 'test'
        expected_argv=['cargo','run','--locked','-q','-p','xtask','--example','c45_baseline','--',kind,needed['label']] if needed['kind']=='command' else needed['argv']
        verify_completed(root,state_folder,state,expected_argv,origin['source_inventory_sha256'],origin_ref['sha256'])
        require(state['exit_code']==0 and state['source_fingerprint']==origin['source_inventory_sha256'] and state['candidate_sha256']==origin_ref['sha256'],'incomplete/stale actual stage')
        original={'execution_id':state['execution_id'],'argv':needed['argv'],'cwd':str(root),'started_utc':state['started_utc'],'finished_utc':state['finished_utc'],'candidate':origin_ref,'required_ids':[cid],'status':'pass','exit_code':0}
        if needed['kind']=='command':
            raw_path=root/state['raw'];raw=read(raw_path)
            commands=[c for c in raw['commands'] if c['label']==needed['label']];require(len(commands)==1,'actual selected runner command missing')
            command=commands[0];original.update(raw=ref(root,raw_path),log=ref(root,raw_path.parent/command['log']))
        else:
            chunk=cid.split('-')[0]
            raw_path=base/('native-acceptance.json' if chunk=='C44' else 'identity/native-acceptance.json' if chunk=='C45' else 'native/cumulative/'+chunk+'/native-acceptance.json')
            raw=read(raw_path);check=next(c for c in raw['checks'] if c['id']==cid)
            original.update(raw=ref(root,raw_path),log=ref(root,state_folder/'stdout.log'),evidence={name:sha(root/name) for name in check['evidence']})
        path=layout['folder']/'bindings'/(cid.replace(':','-').replace('/','-')+'.json');path.parent.mkdir(exist_ok=True)
        if path.exists():require(read(path)==original,'preserve earlier differing result binding')
        else:path.write_text(json.dumps(original,indent=2)+'\n')
        rows.append({'id':cid,'disposition':'reused_exact' if reused else 'executed','result':ref(root,path)})
    record={'schema':'airs-desktop/qualified-coverage/1','subject':'C45','phase':'P10','source_inventory_sha256':source,'candidate':candidate_ref,'plan':ref(root,layout['plan']),'coverage':rows,'meaning':'Actual current C45 baseline executions on their original times/cwd/candidate. P10 consumes these unchanged; no new execution is inferred from a review/merge.'}
    result=verify(root,record)
    out=root/'docs/planning/evidence/C45/qualified-coverage.json';require(not out.exists(),'preserve qualified record')
    out.write_text(json.dumps(record,indent=2)+'\n');(base/'qualification-audit.json').write_text(json.dumps(result,indent=2)+'\n')
    print(json.dumps(result,indent=2))


if __name__=='__main__':finish(Path.cwd().resolve())
