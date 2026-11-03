"""Immutable actual-stage bindings for recovery and finalization."""
from pathlib import Path
from qualification import read,sha,require


def collect(root,base,stage,raw_runner=None):
    files=[];missing=[]
    if raw_runner:
        path=root/raw_runner;raw=read(path);files=[path]+[path.parent/command['log'] for command in raw['commands']]
    elif stage.startswith('native-'):
        chunk=stage[len('native-'):].upper()
        folder=base if chunk=='C44' else base/'identity' if chunk=='C45' else base/'native/cumulative'/chunk
        path=folder/'native-acceptance.json';raw=read(path);files=[path]
        for check in raw['checks']:
            for name in check['evidence']:
                evidence=root/name
                if not evidence.exists() and check['result']!='pass':missing.append(name)
                else:
                    require(evidence.is_file(),'native raw evidence missing: '+name)
                    files.append(evidence)
        if chunk!='C45':
            wrapper=folder/'execution.json';execution=read(wrapper)
            require(execution['raw_record_sha256']==sha(path) and execution['controller_sha256']==sha(root/'tools/p10/native.py') and execution['required_check_errors']==[],'native wrapper/raw/controller binding')
            require(execution['executable_sha256']==read(base/'candidate.json')['executable_sha256'],'native wrapper candidate binding')
            files.append(wrapper)
    elif stage=='startup-smoke':files=list((base/'startup-smoke-01').rglob('*.json'))
    elif stage=='build':files=[root/'docs/planning/evidence/C45/native-prepare.json']
    elif stage=='finalize':files=[root/'docs/planning/evidence/C45/qualified-coverage.json',base/'qualification-audit.json']
    return {**{str(path.relative_to(root)):sha(path) for path in sorted(set(files))},**{name:None for name in missing}}


def verify_completed(root,folder,state,argv,source,candidate_sha=None):
    require(state['exit_code']==0 and state['argv']==argv and state['source_fingerprint']==source,'completed stage command/source failed or differs')
    require(sha(folder/'stdout.log')==state['stdout_sha256'] and sha(folder/'stderr.log')==state['stderr_sha256'],'completed stage log drift')
    if candidate_sha:require(state['candidate_sha256']==candidate_sha,'completed stage candidate drift')
    require(state.get('artifacts'),'completed stage raw artifact binding missing')
    for name,expected in state['artifacts'].items():
        if expected is None:require(not (root/name).exists(),'recorded missing superseded raw evidence changed: '+name)
        else:require(sha(root/name)==expected,'completed stage raw/evidence drift: '+name)
    return True


def verify_conditions(candidate,current,platform_name,hostname):
    require({key:current[key] for key in ['source','dependencies','build_flags']}==candidate['runner_inputs'],'dependency/toolchain/build identity changed; diagnose affected checks without blanket replay')
    require(candidate['conditions']['platform']==platform_name and candidate['conditions']['hostname']==hostname,'execution platform/conditions changed')
    return True
