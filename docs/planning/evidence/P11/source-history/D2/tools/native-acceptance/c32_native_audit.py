"""Aggregate immutable executed C32 facts, preserving every raw failed result.

Attempt02 completed all scenarios but an all-or-nothing reporting rule marked
all three groups failed for two missing assertions. Attempt03 executes only
those two assertions. This is a new evidence audit, never a relabel of either
raw execution or an independent review verdict.
"""
from pathlib import Path
import copy
import hashlib
import json
import sys
sys.path.insert(0,str(Path(__file__).resolve().parent))
from c32_coverage import problems

PREFIX='docs/planning/evidence/C32/remediation-attempt-21/'

def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def read(p):return json.loads(p.read_text())

def report(root):
    root=Path(root)
    original=root/PREFIX/'native-attempt-02'
    follow=root/PREFIX/'native-attempt-03'
    raw=read(original/'native-acceptance.json')
    obs=read(original/'native/observations.json')['observations']
    extra=read(follow/'native.staging/followup-observations.json')
    assert raw['app']['executable_sha256']==extra['app']['executable_sha256']
    assert raw['app']['identifier']==extra['app']['identifier']
    assert extra['keyboard']['before']=={'role':'AXTextArea','label':'Message'}
    assert extra['keyboard']['after']=={'role':'AXButton','label':'Send'}
    keyboard_tree=read(root/extra['keyboard']['tree'])
    assert any(n.get('focused') is True and n.get('role')=='AXButton' and n.get('label')=='Send' for n in keyboard_tree)
    ended_tree=read(root/extra['ended_status']['tree'])
    regions=[]
    for i,n in enumerate(ended_tree):
        if n.get('subrole')!='AXApplicationStatus':continue
        parts=[]
        for m in ended_tree[i+1:]:
            if m.get('depth',0)<=n.get('depth',0):break
            parts.append(str(m.get('label') or m.get('value') or ''))
        regions.append(' '.join(parts))
    assert regions==extra['ended_status']['status_regions'] and any('ended' in s.lower() for s in regions)
    assert all(extra['ended_status'][k] is True for k in ['ended_announced','engine_gone','saved_transcript','rail_ended'])
    combined=copy.deepcopy(obs)
    combined['n3']['ended_announced']=extra['ended_status']['ended_announced']
    combined['n3']['status_regions']=regions
    combined['n4']['send_reached']=extra['keyboard']['send_reached']
    combined['n4']['walk']=[extra['keyboard']['before']['label'],extra['keyboard']['after']['label']]
    errors=problems(combined)
    assert not errors,errors
    originals={c['id']:c for c in raw['checks']}
    assert originals['C32-N01']['result']=='pass'
    evidence=[str(p.relative_to(root)) for directory in [original,follow] for p in directory.rglob('*') if p.is_file() and p.suffix in ('.json','.png','.log')]
    checks=[{'id':cid,'description':c['description'],'result':'pass','observation':'New aggregate audit of actually executed native facts; original raw result '+c['result']+' remains unchanged. See exact source records and follow-up facts.','evidence':sorted(evidence)} for cid,c in originals.items()]
    return {'schema':'airs-v2/c32-native-coverage-audit/1','subject':'C32','method':'read-only source-bound aggregation of separately executed native assertions','app':raw['app'],'checks':checks,'raw_results':{cid:c['result'] for cid,c in originals.items()},'raw_record':str((original/'native-acceptance.json').relative_to(root)),'raw_record_sha256':sha(original/'native-acceptance.json'),'observations_sha256':sha(original/'native/observations.json'),'followup_record':str((follow/'native.staging/followup-observations.json').relative_to(root)),'followup_sha256':sha(follow/'native.staging/followup-observations.json'),'facts':combined,'limitations':['Original attempt02 raw failing IDs and driver exit1 remain unchanged. Only its ended-status and composer-keyboard assertions were freshly executed in attempt03; all other completed facts retain their original provenance. No application rebuild, unrelated test/native rerun, owner waiver, independent score or spoken VoiceOver result is inferred.']}

if __name__=='__main__':
    root=Path.cwd();value=report(root);out=root/PREFIX/'coverage/native-acceptance.json';out.parent.mkdir(exist_ok=True)
    with out.open('x') as f:json.dump(value,f,indent=2);f.write('\n')
    print(json.dumps({'checks':{c['id']:c['result'] for c in value['checks']},'raw_results_unchanged':value['raw_results']}))
