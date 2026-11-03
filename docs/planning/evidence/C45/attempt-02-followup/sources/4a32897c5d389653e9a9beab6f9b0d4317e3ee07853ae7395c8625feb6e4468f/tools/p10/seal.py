"""Create the one C45 baseline seal after focused preflight; never overwrite."""
from pathlib import Path
import json,sys
from candidate import source_inventory,digest,helper_closure
from qualification import read,sha,require
sys.path.insert(0,str(Path.cwd()/'docs/planning/evidence/P09/native-controller'))
from native_requirements import load_requirements

DRIVERS=['c36','c43','c32','c31','c39','c11','c10','c03','c04','c05','c06','c07','c24','c35','c37','c38','c40','c41','c42','c44']
LEGACY={'c03','c04','c05','c06','c07','c10','c11','c24'}


def command_plan(root):
    manifest=read(root/'docs/architecture/checks.json')
    commands=[dict(label='common/'+str(i+1),argv=argv,expects_tests=('vitest' in argv or argv[:2]==['cargo','test'])) for i,argv in enumerate(manifest['common_commands'])]
    chunk=manifest['chunks']['C45']
    tests=[('fixture',chunk['fixture_command'])]+[('host/'+str(i+1),argv) for i,argv in enumerate(chunk['host_commands'])]+[('frontend/'+str(i+1),argv) for i,argv in enumerate(chunk['frontend_commands'])]
    for label,argv in tests:
        if not any(item['argv']==argv for item in commands):commands.append(dict(label='C45/'+label,argv=argv,expects_tests=True))
    return commands


def verify_plan(root, plan):
    expected={}
    for command in command_plan(root):
        expected['CHECK:'+command['label']]=dict(kind='command',argv=command['argv'],label=command['label'],expects_tests=command['expects_tests'])
    native=load_requirements(root)
    for driver in DRIVERS+['c45']:
        argv=['python3','tools/p10/native.py',driver]+(['legacy'] if driver in LEGACY else [])
        if driver=='c45':argv=['python3','tools/native-acceptance/acceptance_c45.py']
        ids=native[driver.upper()] if driver!='c45' else {r['id'] for r in read(root/'docs/architecture/native-checks.json')['chunks']['C45']}
        for cid in ids:expected[cid]=dict(kind='native',argv=argv)
    rows=plan['requirements']
    require(len(rows)==len(expected) and {r['id'] for r in rows}==set(expected),'required plan omits/duplicates/introduces approved coverage')
    for row in rows:
        require(all(row.get(k)==v for k,v in expected[row['id']].items()),'plan command/raw-kind differs from approved source manifest')
    return True


def make(root):
    base=root/'docs/planning/evidence/C45/baseline-02';base.mkdir(exist_ok=True)
    require(not (base/'source-seal.json').exists(),'preserve previous seal')
    requirements=[]
    for command in command_plan(root):
        requirements.append(dict(id='CHECK:'+command['label'],kind='command',**command,affected=True,must_execute=True,dependencies=['current approved manifest target'],helper_paths=[]))
    native=load_requirements(root)
    all_helpers={}
    for driver in DRIVERS+['c45']:
        module='acceptance_c32_current' if driver=='c32' else 'acceptance_'+driver
        closure=helper_closure(root,module);all_helpers.update(closure)
        argv=['python3','tools/p10/native.py',driver]+(['legacy'] if driver in LEGACY else [])
        if driver=='c45':argv=['python3','tools/native-acceptance/acceptance_c45.py']
        ids=native[driver.upper()] if driver!='c45' else {row['id'] for row in read(root/'docs/architecture/native-checks.json')['chunks']['C45']}
        for cid in sorted(ids):requirements.append(dict(id=cid,kind='native',argv=argv,affected=True,must_execute=True,dependencies=['current fixture application and relevant native helpers'],helper_paths=sorted(closure)))
    plan={'schema':'airs-desktop/required-coverage/1','subject':'C45','phase':'P10','baseline_count':1,'requirements':requirements,'commands':command_plan(root),'drivers':DRIVERS,'legacy_drivers':sorted(LEGACY),'helper_closure':all_helpers,'reason':'Owner-approved broad identity/isolation/cleanup baseline. Future fixes/merges/reviews use affected targets with explicit artifact/input proof; no automatic broad repeat.','historical_only':['C30 owner standing/performance','L19 charged3/remaining0','P09 actual all9/G11 and older approved retirements'],'excluded_private_operations':['classic login-Keychain probe','search-list inspection; current four services covered by disposable FILE Keychain test']}
    source=source_inventory(root)
    for name,value in [('source-seal.json',{'files':source,'source_fingerprint':digest(source)}),('required-coverage.json',plan)]:(base/name).write_text(json.dumps(value,indent=2)+'\n')
    print('Sealed',len(source),'source entries;',len(requirements),'required coverage IDs;',len(plan['commands']),'repository commands; native20+C45')


if __name__=='__main__':make(Path.cwd().resolve())
