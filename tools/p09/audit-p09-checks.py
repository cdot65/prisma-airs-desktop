from pathlib import Path
import datetime,hashlib,json,subprocess,sys
root=Path.cwd().resolve();attempt=sys.argv[1]
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
# Driver-only corrections are separate from the application build inputs:
# C43 restores coverage; C11 reads WebKit's split cache counter text within its
# AX region, preserving the zero-count, seal-removal and fresh-read assertions.
# C04 uses the shared current-shell active-tenant verification and saves its AX
# tree instead of requiring the superseded legacy shell's active-tenant text.
# C05 joins the hosted knowledge-check result paragraph's split inline AX text.
# C06 observes exact Home level/XP tokens and certification text in named regions.
# C07 reads the exact signed-in sentence within Academy account and retains AX.
# C24 reads retention and numeric count paragraphs within Diagnostics only.
# C37 verifies the current selected product navigation plus exact tabs/heading.
allowed={'tools/native-acceptance/acceptance_c43.py','tools/native-acceptance/acceptance_c11.py','tools/native-acceptance/acceptance_c04.py','tools/native-acceptance/acceptance_c05.py','tools/native-acceptance/acceptance_c06.py','tools/native-acceptance/acceptance_c07.py','tools/native-acceptance/acceptance_c24.py','tools/native-acceptance/acceptance_c37.py','tools/native-acceptance/legacy_assertions.py','tools/native-acceptance/wizard_coverage.py'}
assert set(changes)<=allowed,changes
prep=json.loads((root/'docs/planning/evidence/P09/native-prepare.json').read_text())
assert sha(root/prep['bundle']/'Contents/MacOS/prisma-airs-desktop-v2')==prep['executable_sha256']
result={'phase':'P09','check_attempt':attempt,'time':datetime.datetime.now(datetime.timezone.utc).isoformat(),
        'check_runs_verified':18,'record_sha256':sha(record),'input_digest':digest,'members':r['inputs']['members'],'distinct_paths':len(current),
        'compiled_inputs_unchanged':True,'fixture_executable_sha256':prep['executable_sha256'],
        'changes_since_fixture_build_inputs':changes,'application_build_inputs_unchanged':True,
        'native_complete':False,'phase_complete':False,
        'limits':'Offline check audit only. Fresh native evidence and independent chunk/phase reviews remain required.'}
out.write_text(json.dumps(result,indent=2)+'\n');print(json.dumps(result,indent=2))
