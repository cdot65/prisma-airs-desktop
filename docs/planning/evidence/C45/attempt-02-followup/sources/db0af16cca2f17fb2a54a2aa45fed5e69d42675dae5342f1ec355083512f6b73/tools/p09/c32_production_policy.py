"""Explicit G10 transition for C32 Harness native-confirmation ownership.

G9 is immutable history. G10 rebuilds and qualifies actual new application
bytes. C31's actual full-nine review remains on its unchanged subject; its
shared file's strictly limited Harness branches are inspected by C32/P09.
"""
from pathlib import Path
import hashlib,json,runpy,subprocess
import phase_inputs as original
import c32_transition_policy as prior
from c31_transition_policy import OWNER_SCORE, OWNER_SHA, BASE_HEAD, BASE_DIGEST
from c32_transition_policy import historical_c29,historical_performance

PREFIX='docs/planning/evidence/C32/remediation-attempt-22/'
ARCHIVE='docs/planning/evidence/P09/native-history/Worf-generation-09-complete-before-C32-scoped-confirmation-20261102/'
BASE_RECORD='docs/planning/evidence/C32/remediation-attempt-21/transition.json'
BASE_SHA='e0655b9f7276faba07416f0ae593ad1b012a9f7870ddf9fd322b31a8f541eb1d'
FIX_PATHS={'crates/desktop-core/src/authority.rs','crates/desktop-core/src/authority/harness_tools.rs','crates/desktop-core/tests/ui_v2_harness/main.rs'}
ADMIN_PATHS={'.gitattributes','docs/planning/evidence/P09/native-controller/run-p09-native-c32.py',*('tools/p09/'+n for n in ['c32_production_policy.py','test_c32_production_policy.py','phase_inputs_c32_production.py','phase_packet_c32_production.py','make-c32-production-packet.py','audit-c32-production-review.py','make-phase-packet-c32-production.py','audit-phase-review-c32-production.py','audit-p09-checks-c32-production.py'])}

def digest(x):return hashlib.sha256((json.dumps(x,sort_keys=True,separators=(',',':'))+'\n').encode()).hexdigest()
def apply_delta(files,delta):
    for n,d in delta.items():
        if d['after'] is None:files.pop(n,None)
        else:files[n]=d['after']
    return files

def block_end(text,start):
    depth=0
    for i in range(start,len(text)):
        if text[i]=='{':depth+=1
        elif text[i]=='}':
            depth-=1
            if depth==0:return i+1
    raise ValueError('unterminated scoped Harness block')

def authority_projection(text):
    import re
    text=re.sub(r'(?m)^    harness_confirming: .*$', '    harness_confirming: C32_NATIVE_OWNER,',text)
    for marker,arm in [('    ConfirmHarness {',False),('        Intent::DecideHarnessApproval {',True),('            Effect::ConfirmHarness {',True)]:
        original.require(text.count(marker)==1,'Harness source marker ambiguous: '+marker)
        start=text.index(marker)
        brace=text.index('=> {',start)+3 if arm else text.index('{',start)
        end=block_end(text,brace)
        text=text[:start]+marker.split('{')[0]+'C32_SCOPED_BLOCK'+text[end:]
    return text.replace(' | Effect::ConfirmHarness { .. }','')

def verify_transition(root):
    from phase_packet_owner_closure import source_inventory
    root=Path(root);proof=original.read(root/PREFIX/'transition.json')
    original.require(original.sha(root/BASE_RECORD)==BASE_SHA==proof['prior_transition_sha256'],'C32 prior transition changed')
    g9root=root/'docs/planning/evidence/P09/c31-transition-20261102'
    base=original.read(g9root/'baseline-source.json')
    original.require(base['digest']==BASE_DIGEST and digest(base['files'])==BASE_DIGEST,'original baseline source changed')
    old=apply_delta(base['files'].copy(),original.read(g9root/'transition-final-01.json')['source_delta'])
    old=apply_delta(old,original.read(root/BASE_RECORD)['source_delta'])
    original.require(digest(old)==original.read(root/BASE_RECORD)['current_source_digest'],'C32 baseline reconstruction mismatch')
    now=source_inventory(root)
    delta={n:{'before':old.get(n),'after':now.get(n)} for n in sorted(set(old)|set(now)) if old.get(n)!=now.get(n)}
    original.require(delta==proof['source_delta'] and set(delta)<=FIX_PATHS|ADMIN_PATHS,'undeclared C32 production delta')
    original.require(digest(now)==proof['current_source_digest'],'current source seal drift')
    original.require(authority_projection((root/ARCHIVE/'reviewed-source/authority.rs').read_text())==authority_projection((root/'crates/desktop-core/src/authority.rs').read_text()),'shared authority changed outside C32 Harness admission/effect ownership')
    historical=runpy.run_path(str(root/ARCHIVE/'reviewed-source/legacy_assertions.py'))['ASSERTIONS'];current=runpy.run_path(str(root/'tools/native-acceptance/legacy_assertions.py'))['ASSERTIONS']
    original.require(set(historical)==set(current) and {n for n in historical if historical[n]!=current[n]}=={'C32-N02','C32-N03','C32-N04'},'non-C32 assertion-map change')
    for item in proof['focused_results']:
        record=original.read(root/item['record']);original.require(record['exit']==0 and original.sha(root/item['record'])==item['sha256'] and original.sha(root/record['log'])==record['log_sha256'],'focused preflight drift')
    original.require(bool(proof['focused_results']),'focused preflight missing')
    original.require(subprocess.run(['git','merge-base','--is-ancestor',proof['fix_commit'],'HEAD'],cwd=root,capture_output=True).returncode==0,'C32 fix not integrated')
    paths=set(subprocess.check_output(['git','diff-tree','--no-commit-id','--name-only','-r',proof['fix_commit']],cwd=root,text=True).splitlines())
    original.require(paths==FIX_PATHS,'C32 fix commit changed unrelated source')
    return {'record':PREFIX+'transition.json','sha256':original.sha(root/PREFIX/'transition.json'),'current_source_digest':digest(now),'changed_paths':sorted(delta),'standing':'Actual G10 current qualification required for C32 production ownership change; unchanged C31 subject retains actual all9 with original scope archived, no re-review or score loop. Source outside declared Harness and administrative delta unchanged.'}

def historical_c31(root):
    root=Path(root);verify_transition(root)
    out=root/'docs/planning/reviews/C31/attempt-07';v=original.read(out/'verdict-returned.json');r=original.read(out/'request.json');a=original.read(out/'post-review-audit.json');receipt=original.read(out/'receipt.json')
    original.full_gate(v)
    for name in ['request.json','verdict-returned.json','post-review-audit.json','receipt.json','source-inventory.json']:
        committed=subprocess.check_output(['git','show','3aaf67ad386fa3817d180ff76ec2d2f76338dd30:'+str((out/name).relative_to(root))],cwd=root)
        original.require(hashlib.sha256(committed).hexdigest()==original.sha(out/name),'C31 committed raw record changed: '+name)
    from phase_inputs_c32_production import verify_launch
    original.require(verify_launch(root,out)==a['transport'],'C31 original independent transport binding')
    original.require(v['request_id']==r['request_id'] and v['fingerprints']==r['fingerprints'],'C31 raw request binding')
    original.require(a['full_gate_pass'] is True and a['raw_verdict_sha256']==original.sha(out/'verdict-returned.json') and a['request_sha256']==original.sha(out/'request.json'),'C31 actual all9 audit binding')
    original.require(receipt['full_gate_pass'] is True and receipt['verdict_sha256']==original.sha(out/'verdict-returned.json') and receipt['post_review_audit_sha256']==original.sha(out/'post-review-audit.json'),'C31 actual receipt binding')
    for name,h in original.read(out/'source-inventory.json')['files'].items():
        p=root/name
        if p.is_file() and original.sha(p)==h:continue
        if name=='crates/desktop-core/src/authority.rs':p=root/ARCHIVE/'reviewed-source/authority.rs'
        elif name=='tools/native-acceptance/legacy_assertions.py':p=root/ARCHIVE/'reviewed-source/legacy_assertions.py'
        elif name=='.gitattributes':p=root/ARCHIVE/'reviewed-source/.gitattributes'
        elif name.startswith('docs/planning/evidence/P09/'):
            p=root/ARCHIVE/name.removeprefix('docs/planning/evidence/P09/')
        original.require(p.is_file() and original.sha(p)==h,'C31 frozen reviewed scope not preserved: '+name)
    return {'subject':'C31','scores':v['scores'],'recommendation':v['recommendation'],'standing':'Historical actual attempt07 full-nine review remains on unchanged C31 subject. Exact scoped inputs retained; shared authority differs only in declared C32 Harness branches, inspected by current C32/P09; no current broad-scope hash-equivalence claim and no owner activation.','record':str((out/'verdict-returned.json').relative_to(root)),'sha256':original.sha(out/'verdict-returned.json'),'request':str((out/'request.json').relative_to(root)),'request_sha256':original.sha(out/'request.json'),'audit':str((out/'post-review-audit.json').relative_to(root)),'audit_sha256':original.sha(out/'post-review-audit.json'),'receipt':str((out/'receipt.json').relative_to(root)),'receipt_sha256':original.sha(out/'receipt.json')}


import owner_closure_policy as baseline
def verify_c30_owner_closure(root):
    root = Path(root).resolve()
    require, read, sha = original.require, original.read, original.sha
    for name, expected in baseline.PINNED.items():
        require(sha(root / name) == expected, 'historical C30 record changed: ' + name)
    latest, verdict = original.latest_verdict(root, 'C30')
    require(latest == root / baseline.ATTEMPT / 'verdict-returned.json', 'C30 has unexpected later review')
    require(verdict['scores'] == baseline.SCORES and verdict['recommendation'] == 'fail', 'raw C30 verdict changed')
    transition = verify_transition(root)
    result = {'subject': 'C30', 'owner_closed': True, 'scores': verdict['scores'],
              'recommendation': 'fail', 'full_gate_pass': False,
              'owner_scores': {key: 9 for key in baseline.SCORES},
              'owner_decision': OWNER_SCORE, 'owner_decision_sha256': OWNER_SHA,
              'transition': transition,
              'standing': 'Owner all-nine C30 standing continues across declared unrelated C32 Harness production delta; raw independent 9/9/9/8/8 remains historical. No C30 requalification or independent passing score claimed.'}
    for key, name in [('record', 'verdict-returned.json'), ('request', 'request.json'),
                      ('receipt', 'receipt.json'), ('audit', 'post-review-audit.json')]:
        result[key] = baseline.ATTEMPT + '/' + name
        result['sha256' if key == 'record' else key + '_sha256'] = sha(root / result[key])
    return result



def bundle_digest(bundle):
    import os
    bundle=Path(bundle);entries=[]
    for path in sorted(bundle.rglob('*')):
        if path.is_dir() and not path.is_symlink():continue
        name=str(path.relative_to(bundle))
        if path.is_symlink():mode='symlink';h=hashlib.sha256(os.readlink(path).encode()).hexdigest()
        else:mode='100755' if path.stat().st_mode & 0o111 else '100644';h=original.sha(path)
        entries.append(f'{name}\0{mode}\0{h}\n')
    return hashlib.sha256(''.join(entries).encode()).hexdigest()
