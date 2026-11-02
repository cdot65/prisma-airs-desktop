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


def verify_transition(root):
    from phase_packet_owner_closure import source_inventory
    root = Path(root).resolve()
    require, read, sha = original.require, original.read, original.sha
    old = read(root / PREFIX / 'baseline-source.json')
    require(old['head'] == BASE_HEAD and digest(old['files']) == old['digest'] == BASE_DIGEST,
            'C31 transition baseline changed')
    seal_path = root / PREFIX / TRANSITION_RECORD
    seal = read(seal_path)
    require(seal['baseline_head'] == BASE_HEAD and seal['baseline_digest'] == BASE_DIGEST,
            'wrong transition baseline')
    require(subprocess.run(['git', 'merge-base', '--is-ancestor', BASE_HEAD, 'HEAD'],
                           cwd=root, capture_output=True).returncode == 0, 'baseline not integrated')
    current = source_inventory(root)
    changed = {name: {'before': old['files'].get(name), 'after': current.get(name)}
               for name in sorted(set(old['files']) | set(current))
               if old['files'].get(name) != current.get(name)}
    require(changed == seal['source_delta'], 'current source differs from declared sealed C31 delta')
    require(set(changed) <= FIX_PATHS | ADMIN_PATHS, 'undeclared non-C31 source change')
    require(digest(current) == seal['current_source_digest'], 'current full source digest mismatch')
    commit_paths = set()
    require(len(seal['fix_commits']) >= 2, 'both collaborator fixes must be declared')
    for commit in seal['fix_commits']:
        require(subprocess.run(['git', 'merge-base', '--is-ancestor', commit, 'HEAD'],
                               cwd=root, capture_output=True).returncode == 0, 'fix commit not integrated')
        names = subprocess.check_output(['git', 'diff-tree', '--no-commit-id', '--name-only', '-r', commit],
                                        cwd=root, text=True).splitlines()
        require(all(name in FIX_PATHS or name.startswith('docs/planning/evidence/C31/remediation-attempt-05/') for name in names), 'fix commit changes unrelated source')
        commit_paths.update(names)
    require(set(changed) & FIX_PATHS <= commit_paths, 'production/driver delta lacks declared fix commit')
    for item in seal['focused_results']:
        focused = read(root / item['record'])
        require(focused['exit'] == 0 and bool(focused['command']), 'focused command did not pass')
        if 'log' in focused:
            require(sha(root / focused['log']) == focused['log_sha256'], 'focused result log changed')
        require(item['exit'] == 0 and sha(root / item['record']) == item['sha256'],
                'focused preflight result missing/changed')
    require(bool(seal['focused_results']), 'no focused preflight results')
    require(sha(root / OWNER_SCORE) == OWNER_SHA, 'owner all-nine decision changed')
    return {'record': PREFIX + TRANSITION_RECORD, 'sha256': sha(seal_path),
            'baseline_head': BASE_HEAD, 'baseline_source_digest': BASE_DIGEST,
            'current_source_digest': digest(current), 'changed_paths': sorted(changed),
            'fix_commits': seal['fix_commits'], 'standing': 'C31-only production and native-driver remediation plus explicit administrative provenance; current qualification required'}


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
              'standing': 'Owner all-nine C30 standing continues across declared unrelated C31 delta; raw independent 9/9/9/8/8 remains historical. No C30 requalification or independent passing score claimed.'}
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


def verify_driver_amendment(root, current_digest):
    """Explicit same-G8 application continuation; never relabel historical runs."""
    import ast
    import os
    from phase_packet_owner_closure import source_inventory
    root = Path(root)
    require, read, sha = original.require, original.read, original.sha
    prefix = PREFIX + 'driver-amendment-01/'
    proof_path = root / prefix / 'equivalence.json'
    proof = read(proof_path)
    old_seal = root / PREFIX / 'transition.json'
    require(sha(old_seal) == proof['original_transition_sha256'] == '39a2fd816437d55fbc163598a4b09f1dcb8f682c7acbe2f57cd1df4f7c070d4d', 'first source seal changed')
    old = read(root / PREFIX / 'baseline-source.json')['files'].copy()
    for name, value in read(old_seal)['source_delta'].items():
        if value['after'] is None:
            old.pop(name, None)
        else:
            old[name] = value['after']
    current = source_inventory(root)
    changed = {name: {'before': old.get(name), 'after': current.get(name)}
               for name in sorted(set(old) | set(current)) if old.get(name) != current.get(name)}
    require(changed == proof['source_delta_since_first_seal'], 'driver amendment source delta drift')
    native_paths = {'tools/native-acceptance/acceptance_c31.py', 'tools/native-acceptance/test_administration_coverage.py'}
    require(set(changed) <= native_paths | ADMIN_PATHS, 'driver continuation changed application or unrelated source')
    def inventory(path):
        return {name.strip(): value for value, name in (line.split(None, 1) for line in path.read_text().splitlines())}
    before_path = root / proof['compiled_inputs_before']
    after_path = root / proof['compiled_inputs_after']
    require(sha(before_path) == proof['before_digest'] and sha(after_path) == proof['after_digest'] == current_digest,
            'driver amendment compiled-input seal drift')
    before, after = inventory(before_path), inventory(after_path)
    input_delta = sorted(name for name in set(before) | set(after) if before.get(name) != after.get(name))
    require(input_delta == proof['compiled_input_changes'] and set(input_delta) <= native_paths,
            'reused G8 application has changed compiled application inputs')
    driver = 'tools/native-acceptance/acceptance_c31.py'
    old_driver = root / prefix / 'original-acceptance_c31.py'
    require(sha(old_driver) == old[driver]['sha256'], 'old driver snapshot does not match first seal')
    def normalized(path):
        tree = ast.parse(path.read_text())
        for node in tree.body:
            if isinstance(node, ast.FunctionDef) and node.name == 'gateway_key_checks':
                node.body = [ast.Pass()]
        return ast.dump(tree, include_attributes=False)
    require(normalized(old_driver) == normalized(root / driver), 'runtime change exceeds the Gateway key-check function')
    callers = set()
    for path in (root / 'tools/native-acceptance').glob('*.py'):
        if path.name.startswith('test_'):
            continue
        tree = ast.parse(path.read_text())
        if any(isinstance(n, ast.Call) and ((isinstance(n.func, ast.Name) and n.func.id == 'gateway_key_checks')
                   or (isinstance(n.func, ast.Attribute) and n.func.attr == 'gateway_key_checks')) for n in ast.walk(tree)):
            callers.add(str(path.relative_to(root)))
    require(callers == {'tools/native-acceptance/acceptance_c31.py', 'tools/native-acceptance/acceptance_c39.py'},
            'changed helper has additional runtime callers')
    prep_path = root / 'docs/planning/evidence/P09/native-prepare.json'
    build_path = root / 'docs/planning/evidence/P09/native-build-inputs.json'
    require(sha(prep_path) == proof['prepare_sha256'] and sha(build_path) == proof['build_record_sha256'],
            'original actual G8 preparation/build record changed')
    prep = read(prep_path)
    require(sha(root / prep['bundle'] / 'Contents/MacOS/prisma-airs-desktop-v2') == prep['executable_sha256'] == proof['executable_sha256'],
            'G8 application executable changed')
    entries = []
    bundle = root / prep['bundle']
    for path in sorted(bundle.rglob('*')):
        if path.is_dir() and not path.is_symlink():
            continue
        if path.is_symlink():
            mode, value = 'symlink', hashlib.sha256(os.readlink(path).encode()).hexdigest()
        else:
            mode, value = ('100755' if path.stat().st_mode & 0o111 else '100644'), sha(path)
        entries.append(f'{path.relative_to(bundle)}\0{mode}\0{value}\n')
    require(hashlib.sha256(''.join(entries).encode()).hexdigest() == prep['bundle_sha256'] == proof['bundle_sha256'],
            'complete G8 bundle changed')
    require(len(proof['completed_native_steps']) == 10, 'wrong retained G8 step count')
    for item in proof['completed_native_steps']:
        for key in ['record', 'execution', 'log']:
            require(sha(root / item[key]) == item[key + '_sha256'], 'retained G8 evidence changed: ' + item[key])
        execution = read(root / item['execution'])
        require(execution['driver_subject'] == item['subject'] and item['subject'] not in ['C31', 'C39', 'C44']
                and execution['required_check_errors'] == [] and execution['executable_sha256'] == proof['executable_sha256'],
                'invalid retained G8 native step')
        require(sha(root / 'tools/native-acceptance' / ('acceptance_' + item['subject'].lower() + '.py')) == execution['driver_sha256'],
                'retained native driver changed')
    return {'record': str(proof_path.relative_to(root)), 'sha256': sha(proof_path),
            'original_build_input_digest': proof['before_digest'], 'current_driver_input_digest': current_digest,
            'retained_native_steps': 10, 'affected_native_steps': ['C31', 'C39', 'C44'],
            'meaning': 'Same current G8 application/full bundle; only Gateway key-check flow and its test changed. Ten original passing records remain unchanged; affected/unrun drivers execute freshly. No G7 result is current and no current C30 timing is claimed.'}
