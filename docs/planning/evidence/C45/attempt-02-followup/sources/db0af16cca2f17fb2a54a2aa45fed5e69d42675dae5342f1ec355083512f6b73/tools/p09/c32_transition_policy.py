"""Explicit C31-only transition from G7; historical qualification is never current.

Original owner_closure_policy.py and all raw reviews remain immutable. The
transition manifest is sealed only after both declared fixes and preflight.
"""
from pathlib import Path
import hashlib
import json
import subprocess
import owner_closure_policy as baseline
import phase_inputs as original

PREFIX = 'docs/planning/evidence/P09/c31-transition-20261102/'
TRANSITION_RECORD = 'transition-final-01.json'
BASE_HEAD = 'e825c2f69cbcd19387142b80481faaad84690a07'
BASE_DIGEST = 'e441459dde8a4b13403f16f16e5abf100636421ff827f9955f37740500bbdc50'
OWNER_SCORE = 'docs/planning/C30-owner-score-override-2026-11-02.md'
OWNER_SHA = 'c150ebb6a35d8b1bdb6965a5780b96b88f393ed2994cdd3a11da2b4d20bd1a5e'
FIX_PATHS = {
    'crates/desktop-core/src/lib.rs',
    'crates/desktop-core/src/presentation.rs',
    'apps/desktop/src-tauri/src/native_confirmation.rs',
    'tools/native-acceptance/mgmt_server.py',
    'crates/desktop-core/src/authority.rs',
    'crates/desktop-core/src/authority/change_domain.rs',
    'crates/desktop-core/src/delivery.rs',
    'crates/desktop-host/src/lib.rs',
    'apps/desktop/src-tauri/src/lib.rs',
    'apps/desktop/src-tauri/Cargo.toml',
    'crates/desktop-host/Cargo.toml',
    'crates/desktop-host/tests/renderer_review_lifecycle.rs',
    'apps/desktop/src-tauri/tests/renderer_review_lifecycle.rs',
    'tools/native-acceptance/acceptance_c31.py',
    'tools/native-acceptance/acceptance_c39.py',
    'tools/native-acceptance/legacy_assertions.py',
    'tools/native-acceptance/administration_coverage.py',
    'tools/native-acceptance/test_administration_coverage.py',
}
ADMIN_PATHS = {
    '.gitattributes',
    'docs/planning/C31-conditional-owner-override-2026-11-02.md',
    'tools/p09/c31_owner_completion.py',
    'tools/p09/test_c31_owner_completion.py',
    'tools/p09/c31_transition_policy.py',
    'tools/p09/phase_inputs_c31_transition.py',
    'tools/p09/phase_packet_c31_transition.py',
    'tools/p09/make-c31-transition-packet.py',
    'tools/p09/audit-c31-transition-review.py',
    'tools/p09/make-phase-packet-c31-transition.py',
    'tools/p09/audit-phase-review-c31-transition.py',
    'tools/p09/seal-c31-transition.py',
    'tools/p09/test_c31_transition.py',
    'tools/p09/audit-p09-checks-c31-transition.py',
    'tools/p09/README.md',
}


def digest(value):
    return hashlib.sha256((json.dumps(value, sort_keys=True, separators=(',', ':')) + '\n').encode()).hexdigest()


C32_PREFIX = 'docs/planning/evidence/C32/remediation-attempt-21/'
C32_NATIVE = C32_PREFIX + 'coverage/'
C32_ALLOWED = {
    'tools/native-acceptance/acceptance_c32_current.py',
    'tools/native-acceptance/c32_coverage.py',
    'tools/native-acceptance/acceptance_c32_followup.py',
    'tools/native-acceptance/c32_native_audit.py',
    'tools/native-acceptance/test_c32_native_audit.py',
    'tools/native-acceptance/test_c32_coverage.py',
    'tools/native-acceptance/legacy_assertions.py',
    *('tools/p09/' + name for name in (
        'c32_transition_policy.py', 'phase_inputs_c32_transition.py',
        'phase_packet_c32_transition.py', 'make-c32-transition-packet.py',
        'audit-c32-transition-review.py', 'make-phase-packet-c32-transition.py',
        'audit-phase-review-c32-transition.py', 'test_c32_transition.py')),
}


def verify_transition(root):
    from phase_packet_owner_closure import source_inventory
    root = Path(root)
    proof = original.read(root / C32_PREFIX / 'transition.json')
    seal_path = root / PREFIX / TRANSITION_RECORD
    original.require(original.sha(seal_path) == proof['g9_transition_sha256'] == '9aaf4c2f8f1bd4f0165ebd0c58e5019ab44706e5b5ccc1cdca0111a01c5a5e67', 'original G9 seal changed')
    old_seal = original.read(seal_path)
    old = original.read(root / PREFIX / 'baseline-source.json')['files'].copy()
    for name, delta in old_seal['source_delta'].items():
        if delta['after'] is None: old.pop(name, None)
        else: old[name] = delta['after']
    original.require(digest(old) == old_seal['current_source_digest'], 'G9 source reconstruction mismatch')
    current = source_inventory(root)
    delta = {n: {'before': old.get(n), 'after': current.get(n)} for n in sorted(set(old) | set(current)) if old.get(n) != current.get(n)}
    original.require(delta == proof['source_delta'] and set(delta) <= C32_ALLOWED, 'undeclared C32 source delta')
    original.require(digest(current) == proof['current_source_digest'], 'current source seal drift')
    # Only C32 entries may change in the shared assertion map; every C30/C31 entry stays identical.
    import runpy
    historical = root / C32_PREFIX / 'g9-legacy-assertions.py'
    original.require(original.sha(historical) == old['tools/native-acceptance/legacy_assertions.py']['sha256'], 'historical G9 map changed')
    before = runpy.run_path(str(historical))['ASSERTIONS']
    after = runpy.run_path(str(root / 'tools/native-acceptance/legacy_assertions.py'))['ASSERTIONS']
    original.require(set(before) == set(after), 'native IDs changed')
    original.require({n for n in before if before[n] != after[n]} == {'C32-N02','C32-N03','C32-N04'}, 'non-C32 mapping changed')
    for item in proof['focused_results']:
        record = original.read(root / item['record'])
        original.require(record['exit'] == 0 and original.sha(root / item['record']) == item['sha256'], 'focused check binding')
        original.require(original.sha(root / record['log']) == record['log_sha256'], 'focused log drift')
    original.require(bool(proof['focused_results']), 'focused checks missing')
    return {'record': C32_PREFIX + 'transition.json', 'sha256': original.sha(root / C32_PREFIX / 'transition.json'), 'current_source_digest': digest(current), 'changed_paths': sorted(delta), 'standing': 'C32-only supplemental native coverage and additive provenance; unchanged production, application bundle, existing driver/helper closures and historical G9 results. C31 actual full-nine remains closed; no C30 work.'}


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
              'standing': 'Owner all-nine C30 standing continues across declared C31 production delta followed by C32-only native coverage delta; raw independent 9/9/9/8/8 remains historical. No C30 requalification or independent passing score claimed.'}
    for key, name in [('record', 'verdict-returned.json'), ('request', 'request.json'),
                      ('receipt', 'receipt.json'), ('audit', 'post-review-audit.json')]:
        result[key] = baseline.ATTEMPT + '/' + name
        result['sha256' if key == 'record' else key + '_sha256'] = sha(root / result[key])
    return result


def historical_c29(root):
    root = Path(root)
    require, read, sha = original.require, original.read, original.sha
    path, verdict = original.latest_verdict(root, 'C29')
    require(verdict['attempt_id'] == 'attempt-10', 'unexpected C29 generation')
    original.full_gate(verdict)
    item = {'subject': 'C29', 'scores': verdict['scores'], 'recommendation': verdict['recommendation'],
            'standing': 'Historical actual full-nine G7 review; shared authority file changes are declared C31-only delta, inspected by new C31 and cumulative P09 reviews. No current-source-equivalence claim.'}
    # These committed audit artifacts remain byte-identical to the baseline.
    for key, name in [('record', 'verdict-returned.json'), ('request', 'request.json'),
                      ('receipt', 'receipt.json'), ('audit', 'post-review-audit.json')]:
        rel = str((path.parent / name).relative_to(root))
        old = subprocess.check_output(['git', 'show', BASE_HEAD + ':' + rel], cwd=root)
        require(hashlib.sha256(old).hexdigest() == sha(root / rel), 'historical C29 record changed')
        item[key] = rel
        item['sha256' if key == 'record' else key + '_sha256'] = sha(root / rel)
    require(read(path.parent / 'post-review-audit.json')['full_gate_pass'] is True, 'C29 original audit not passing')
    return item


def historical_performance(root):
    """Retain historical performance honestly; owner explicitly forbids new C30 work."""
    root = Path(root)
    rel = 'docs/planning/evidence/P09/workload-perf-C30.json'
    old = subprocess.check_output(['git', 'show', BASE_HEAD + ':' + rel], cwd=root)
    original.require(hashlib.sha256(old).hexdigest() == original.sha(root / rel), 'historical C30 performance changed')
    return {'record': rel, 'sha256': original.sha(root / rel), 'current_qualification': False,
            'standing': 'Historical G7 measurement retained under owner C30 closure; not a measurement of current C31 application bytes; no new C30 scenario or timing run authorized.'}


def baseline_requirements(root):
    import re, runpy
    root = Path(root)
    manifest = original.read(root / 'docs/architecture/native-checks.json')['chunks']
    data = runpy.run_path(str(root / C32_PREFIX / 'g9-legacy-assertions.py'))
    assertions = data['ASSERTIONS']
    expected = {c['id'] for chunk, checks in manifest.items() if re.fullmatch(r'C(0[1-9]|1\d|2\d|3[0-3])', chunk) for c in checks}
    original.require(set(assertions) == expected, 'historical native map membership')
    targets = {t for kind, ts, _ in assertions.values() if kind in (data['X'],data['S'],data['P']) for t in ts}
    self_run = {lid.split('-')[0] for lid,(kind,_,_) in assertions.items() if kind == data['S']}
    required = {c:{t for t in targets if t.startswith(c+'-')} for c in self_run}
    required = {c:ts for c,ts in required.items() if ts}
    for c in [f'C{i}' for i in range(35,45)]: required[c] = {x['id'] for x in manifest[c]}
    original.require(len(required) == 19, 'G9 original driver count')
    return required


def validate_input_delta(old, now, declared):
    changed = {n for n in set(old)|set(now) if old.get(n)!=now.get(n)}
    original.require(changed == C32_ALLOWED & set(now) & set(declared), 'unexpected compiled-input changes')
    return changed


def require_bundle_equivalence(expected, actual):
    original.require(actual == expected, 'full bundle changed')


def verify_driver_amendment(root, current_digest):
    import subprocess,tempfile
    root = Path(root)
    transition = verify_transition(root)
    proof = original.read(root / C32_PREFIX / 'transition.json')
    prepare = original.read(root / 'docs/planning/evidence/P09/native-prepare.json')
    original.require(original.sha(root / 'docs/planning/evidence/P09/native-prepare.json') == proof['g9_prepare_sha256'], 'application preparation changed')
    original.require(original.sha(root / prepare['bundle'] / 'Contents/MacOS/prisma-airs-desktop-v2') == prepare['executable_sha256'] == proof['executable_sha256'], 'application bytes changed')
    # Complete bundle identity includes modes and symlink destinations, not just the executable.
    from phase_packet_owner_closure import file_identity
    bundle = root / prepare['bundle']
    actual = {str(p.relative_to(bundle)):file_identity(p) for p in bundle.rglob('*') if p.is_file() or p.is_symlink()}
    require_bundle_equivalence(proof['bundle_inventory'], actual)
    old_path = root / 'docs/planning/evidence/P09/runs/attempt-09/inputs-before.sha256'
    old = dict((line.split(None,1)[1],line.split(None,1)[0]) for line in old_path.read_text().splitlines())
    with tempfile.TemporaryDirectory(prefix='c32-inputs-') as work:
        path = Path(work)/'inputs.sha256'
        observed = subprocess.check_output(['/bin/bash', str(root/'docs/planning/evidence/P09/runs/hash-inputs.sh'),str(root),str(path)],text=True).strip()
        now = dict((line.split(None,1)[1],line.split(None,1)[0]) for line in path.read_text().splitlines())
    original.require(observed == current_digest, 'current input digest mismatch')
    validate_input_delta(old, now, proof['source_delta'])
    original.require(current_digest == proof['current_input_digest'], 'sealed compiled-input drift')
    return {'transition': transition,'original_g9_input_digest':original.sha(old_path),'current_input_digest':current_digest,'reuse_reason':'Only new C32 Python driver/validator/tests and C32 map entries changed. Rust, TypeScript, fixture/service helpers and actual complete bundle unchanged. G9 native19/check18 retained unchanged, not relabeled; new C32 native executes affected coverage.'}


def verify_c32_supplement(root, prepared):
    import datetime,sys
    root=Path(root)
    from phase_packet_owner_closure import file_identity
    sys.path.insert(0,str(root/'tools/native-acceptance'))
    import c32_native_audit
    path=root/C32_NATIVE/'native-acceptance.json'
    record=original.read(path)
    original.require(record==c32_native_audit.report(root), 'C32 evidence aggregation drift')
    original.require(record['app']['executable_sha256']==prepared['executable_sha256'], 'C32 application binding')
    intervals=[]
    for attempt,expected_exit,driver,raw_name in [('02',1,'acceptance_c32_current.py','native-acceptance.json'),('03',0,'acceptance_c32_followup.py','native.staging/followup-observations.json')]:
        directory=root/C32_PREFIX/('native-attempt-'+attempt)
        execution=original.read(directory/'execution.json')
        owner=original.read(directory/'owner.json')
        original.require(execution['exit']==expected_exit and execution['raw_record_sha256']==original.sha(directory/raw_name), 'C32 raw execution/result binding')
        driver_source=directory/'driver-source.py' if attempt=='02' else root/'tools/native-acceptance'/driver
        original.require(execution['driver_sha256']==original.sha(driver_source), 'C32 executed driver changed')
        original.require(execution['controller_sha256']==original.sha(directory/'native-job.py'), 'C32 controller changed')
        original.require(execution['log_sha256']==original.sha(directory/'driver.log'), 'C32 raw log changed')
        original.require(execution['source_seal_sha256']==original.sha(directory/'source-seal.json')==owner['source_seal_sha256'], 'C32 source/owner binding')
        seal=original.read(directory/'source-seal.json')
        original.require(digest(seal['files'])==seal['digest'], 'C32 execution source digest')
        for name,identity in seal['files'].items():
            if attempt=='02' and name=='tools/native-acceptance/acceptance_c32_current.py':
                original.require(identity['sha256']==original.sha(driver_source), 'C32 original driver source mismatch')
            else: original.require(file_identity(root/name)==identity, 'source drift since native execution: '+name)
        original.require(any('Terminal.app/Contents/MacOS/Terminal' in p['command'] for p in owner['terminal_ancestry']), 'C32 job not Terminal owned')
        original.require(execution['source_root']==str(root) and execution['argv']==[execution['python'],'-I','tools/native-acceptance/'+driver], 'C32 command binding')
        start=datetime.datetime.fromisoformat(execution['started_utc']);end=datetime.datetime.fromisoformat(execution['finished_utc'])
        original.require(start<=end, 'C32 execution interval')
        intervals.append((start,end))
    old_end=max(datetime.datetime.fromisoformat(original.read(root/('docs/planning/evidence/P09/execution.json' if c=='C44' else f'docs/planning/evidence/P09/native/cumulative/{c}/execution.json'))['finished_utc']) for c in baseline_requirements(root))
    original.require(old_end<=intervals[0][0] and intervals[0][1]<=intervals[1][0], 'native UI writers overlap')
    return {'subject':'C32','record':str(path.relative_to(root)),'sha256':original.sha(path),'required_targets':sorted(c['id'] for c in record['checks']),'executable_sha256':prepared['executable_sha256'],'raw_results_preserved':record['raw_results'],'standing':'New source-bound aggregation of actual separate native executions; raw failed reporting retained unchanged, not relabeled.'}
