from pathlib import Path
import datetime,hashlib,json,subprocess,sys
root=Path.cwd().resolve();attempt=sys.argv[1]
sys.path.insert(0,str(root/'tools/p09'))
import phase_inputs_c31_transition as phase_inputs
record=root/f'docs/planning/evidence/P09/runs/run-{attempt}.json'
r=json.loads(record.read_text());out=record.parent/attempt/'post-check-audit.json'
assert not out.exists(),'preserve prior post-check audits'
sha=lambda p:hashlib.sha256(Path(p).read_bytes()).hexdigest()
assert len(r['runs'])==18
for name,run in r['runs'].items():
    assert run['exit']==0,name
    assert sha(root/run['log'])==run['log_sha256'],name
assert r['inputs']['unchanged'] and r['inputs']['before_digest']==r['inputs']['after_digest']
scratch=root/'.evidence-private/p09-review'
scratch.mkdir(parents=True,exist_ok=True)
post=scratch/f'p09-post-{attempt}-inputs.sha256'
digest=subprocess.check_output(['/bin/bash',str(root/'docs/planning/evidence/P09/runs/hash-inputs.sh'),str(root),str(post)],text=True).strip()
assert digest==r['inputs']['before_digest'],'compiled input drift'
def inv(p):
    pairs=[line.split(None,1) for line in Path(p).read_text().splitlines()]
    return {name.strip():digest for digest,name in pairs}
built=inv(root/'docs/planning/evidence/P09/runs/attempt-01/inputs-after.sha256');current=inv(post)
changes=[p for p in sorted(built.keys()|current.keys()) if built.get(p)!=current.get(p)]
# The old preparation comparison is retained as provenance, never an exemption
# list. Every current compiled input must match the fresh native build's actual
# before/after seal, and all mapped native records must bind that executable.
qualification=phase_inputs.verify_native(root)
assert qualification['build_inputs']['input_digest']==digest,'checks differ from native build inputs'
prep=json.loads((root/'docs/planning/evidence/P09/native-prepare.json').read_text())
assert sha(root/prep['bundle']/'Contents/MacOS/prisma-airs-desktop-v2')==prep['executable_sha256']
result={'phase':'P09','check_attempt':attempt,'time':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'check_runs_verified':18,'record_sha256':sha(record),'input_digest':digest,'members':r['inputs']['members'],'distinct_paths':len(current),
        'compiled_inputs_unchanged':True,'fixture_executable_sha256':prep['executable_sha256'],
        'changes_since_historical_preparation_inputs':changes,
        'current_native_build_inputs_match':True,'native_build_inputs':qualification['build_inputs'],
        'native_drivers_verified':len(qualification['records']),
        'performance':qualification['performance'],
        'native_complete':True,'phase_complete':False,
        'limits':'Current check/native input audit only. Actual independent chunk and phase reviews remain required.'}
out.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
