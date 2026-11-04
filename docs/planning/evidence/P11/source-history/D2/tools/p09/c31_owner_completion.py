"""Owner's single final C31 review cap; raw independent results never change."""
from pathlib import Path
import hashlib
import json
import phase_inputs as original

DECISION = 'docs/planning/C31-conditional-owner-override-2026-11-02.md'
DECISION_SHA = '210576fcfce83b3c1862318df06330054cd302bfafb5e1815ec94a2938240291'
ACTIVATION = 'docs/planning/reviews/C31/owner-activation-2026-11-02.json'
DIMENSIONS = {'architecture', 'behavior', 'authority_security', 'validation', 'overall'}


def first_final_verdict(root):
    for directory in sorted((Path(root) / 'docs/planning/reviews/C31').glob('attempt-*'),
                            key=lambda p: int(p.name.split('-')[-1])):
        if int(directory.name.split('-')[-1]) <= 6:
            continue
        for name in ['verdict-returned.json', 'verdict.json']:
            path = directory / name
            if path.is_file():
                value = original.read(path)
                original.require(value.get('subject') == 'C31' and value.get('attempt_id') == directory.name,
                                 'wrong C31 final verdict identity')
                scores = value.get('scores', {})
                original.require(set(scores) == DIMENSIONS and all(type(n) is int and 1 <= n <= 10 for n in scores.values()),
                                 'final C31 verdict has no actual numeric scores')
                return path, value
    raise ValueError('no completed actual C31 verdict after attempt-06; transport failure is not a score')


def bindings(root):
    root = Path(root)
    require, read, sha = original.require, original.read, original.sha
    require(sha(root / DECISION) == DECISION_SHA, 'C31 conditional owner decision changed')
    path, verdict = first_final_verdict(root)
    require(any(n < 9 for n in verdict['scores'].values()), 'owner condition not triggered by actual scores')
    directory = path.parent
    request = read(directory / 'request.json')
    audit = read(directory / 'post-review-audit.json')
    receipt = read(directory / 'receipt.json')
    for key in ['subject', 'attempt_id', 'request_id', 'fingerprints']:
        require(request[key] == verdict[key], 'C31 final request/verdict mismatch: ' + key)
    policy = request['c31_final_review_policy']
    require(policy['decision'] == DECISION and policy['decision_sha256'] == DECISION_SHA
            and policy['last_completed_at_direction'] == 'attempt-06', 'final review did not bind owner cap')
    require(DECISION in request['scope']['files'], 'owner cap not frozen in final review source')
    require(audit['raw_verdict_sha256'] == sha(path) and audit['request_sha256'] == sha(directory / 'request.json')
            and audit['scores'] == verdict['scores'] and audit['full_gate_pass'] is False
            and audit['compiled_inputs_unchanged'] is True, 'C31 raw audit missing or altered')
    require(receipt['verdict_sha256'] == sha(path) and receipt['request_sha256'] == sha(directory / 'request.json')
            and receipt['post_review_audit_sha256'] == sha(directory / 'post-review-audit.json')
            and receipt['scores'] == verdict['scores'] and receipt['recommendation'] == verdict['recommendation']
            and receipt['fingerprints'] == verdict['fingerprints'] and receipt['full_gate_pass'] is False,
            'C31 raw receipt missing or altered')
    for key in ['required_fixes', 'blockers', 'evidence_gaps']:
        require(receipt[key] == verdict[key], 'accepted C31 findings changed: ' + key)
    inventory = read(directory / 'source-inventory.json')
    require(set(inventory['files']) == set(request['scope']['files']), 'C31 final frozen scope membership drift')
    for name, value in inventory['files'].items():
        require(sha(root / name) == value, 'C31 final reviewed input drift: ' + name)
    aggregate = hashlib.sha256(''.join(f'{name} {value}\n' for name, value in sorted(inventory['files'].items())).encode()).hexdigest()
    require(aggregate == inventory['source_fingerprint'] == verdict['fingerprints']['source'], 'C31 final source aggregate drift')
    require(audit['transport'] == original.verify_launch(root, directory), 'C31 final actual transport binding drift')
    transition = request['source_transition']
    require(sha(root / transition['record']) == transition['sha256'], 'C31 final remediation source seal drift')
    import phase_inputs_c31_transition as current
    from c31_transition_policy import verify_transition
    require(transition == verify_transition(root), 'C31 final qualified source transition drift')
    qualified = current.verify_native(root)
    require(qualified['build_inputs']['input_digest'] == request['fingerprints']['dependencies'],
            'C31 final native/check input binding drift')
    current.verify_checks(root, root / request['commands'][0]['record'])
    records = {key: {'record': str((directory / name).relative_to(root)), 'sha256': sha(directory / name)}
               for key, name in [('verdict', path.name), ('request', 'request.json'), ('source_inventory', 'source-inventory.json'),
                                 ('audit', 'post-review-audit.json'), ('receipt', 'receipt.json'),
                                 ('invocation', 'invocation.json'), ('thread_reply', 'thread-reply.log')]}
    return {'decision': DECISION, 'decision_sha256': DECISION_SHA, 'attempt': verdict['attempt_id'],
            'request_id': verdict['request_id'], 'actual_scores': verdict['scores'],
            'actual_recommendation': verdict['recommendation'],
            'accepted_findings': {key: verdict[key] for key in ['required_fixes', 'blockers', 'evidence_gaps']},
            'fingerprints': verdict['fingerprints'], 'source_transition': transition, 'bindings': records,
            'transport': audit['transport'], 'qualified_native': qualified,
            'check_record': request['commands'][0]['record'],
            'check_record_sha256': sha(root / request['commands'][0]['record']),
            'owner_scores': {key: 9 for key in DIMENSIONS}, 'raw_full_gate_pass': False,
            'owner_complete': True, 'further_c31_cycles_authorized': False}


def activate(root):
    root = Path(root)
    value = bindings(root)
    path = root / ACTIVATION
    with path.open('x') as output:
        json.dump(value, output, indent=2); output.write('\n')
    return verify(root)


def verify(root):
    root = Path(root)
    value = bindings(root)
    original.require(original.read(root / ACTIVATION) == value, 'C31 owner activation binding changed')
    records = value['bindings']
    return {'subject': 'C31', 'owner_closed': True, 'owner_complete': True, 'full_gate_pass': False,
            'scores': value['actual_scores'], 'owner_scores': value['owner_scores'],
            'recommendation': value['actual_recommendation'], 'accepted_findings': value['accepted_findings'],
            'owner_decision': DECISION, 'owner_decision_sha256': DECISION_SHA,
            'activation': ACTIVATION, 'activation_sha256': original.sha(root / ACTIVATION),
            'record': records['verdict']['record'], 'sha256': records['verdict']['sha256'],
            **{key: records[key]['record'] for key in ['request', 'audit', 'receipt']},
            **{key + '_sha256': records[key]['sha256'] for key in ['request', 'audit', 'receipt']},
            'standing': 'Owner conditional C31 all-nine completion activated by the first actual post-fix numeric verdict after attempt06; raw score/findings/blockers/gaps/recommendation/audit unchanged. No further C31 scoring cycle or reopening these findings through P09 is authorized. C32/P09 still require actual independent full-nine gates.'}
