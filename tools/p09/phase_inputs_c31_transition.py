"""Current qualification after explicitly sealed C31 remediation.

C29/C30 historical standing is disclosed; no old application qualification is
transferred to current bytes. Original owner-closure verifier remains unchanged.
"""
from pathlib import Path
import datetime
import hashlib
import json
import math
import re
import subprocess
import sys
import tempfile

from c31_transition_policy import (verify_c30_owner_closure, verify_transition, historical_c29, historical_performance)

MEMBERS = ['C27', 'C28', 'C29', 'C30', 'C31', 'C32', 'C33', *[f'C{i}' for i in range(35, 45)]]
DECISIONS = {
    'C33': 'docs/planning/p09-recut.md',
    'C41': 'docs/planning/reviews/C41/owner-override-2026-10-31.md',
}
DIMENSIONS = {'architecture', 'behavior', 'authority_security', 'validation', 'overall'}
REVIEW_DISABLED = {'hooks', 'plugins', 'apps', 'remote_plugin', 'browser_use', 'browser_use_external',
                   'in_app_browser', 'computer_use', 'image_generation', 'multi_agent', 'skill_search',
                   'skill_mcp_dependency_install', 'unbounded_connection_retries'}


def require(condition, message):
    if not condition:
        raise ValueError(message)


def read(path):
    return json.loads(Path(path).read_text())


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def full_gate(verdict):
    scores = verdict.get('scores', {})
    require(set(scores) == DIMENSIONS and all(type(v) is int and v >= 9 and v <= 10 for v in scores.values()),
            'member does not have all five scores at least 9')
    require(verdict.get('recommendation') == 'pass', 'member recommendation is not pass')
    for field in ['required_fixes', 'blockers', 'evidence_gaps']:
        require(verdict.get(field) == [], 'member has missing/nonempty ' + field)


def verify_launch(root, out):
    """Audit requested isolation, observed feature probe and actual JSONL events."""
    invocation = read(out / 'invocation.json')
    require(invocation['request_sha256'] == sha(out / 'request.json')
            and invocation['schema_sha256'] == sha(root / 'docs/architecture/review-verdict.schema.json'),
            'review invocation request/schema mismatch')
    argv = invocation['command_without_prompt']
    require(argv and argv[0] == 'codex' and argv.count('exec') == 1, 'review is not a fresh Codex exec')
    for flag in ['--no-daemon', '--ephemeral', '--ignore-user-config', '--ignore-rules', '--json']:
        require(argv.count(flag) == 1, 'review isolation flag missing/duplicated: ' + flag)
    for flag, value in [('-a', 'never'), ('-s', 'read-only'), ('-m', invocation['model_requested']),
                        ('--output-schema', 'docs/architecture/review-verdict.schema.json'),
                        ('-o', str(out / 'verdict-returned.json'))]:
        require(argv.count(flag) == 1 and argv[argv.index(flag) + 1] == value, 'review argument mismatch: ' + flag)
    require(not any('dangerously' in x or x in ['--approve-for-me', 'resume', 'fork', '--last'] for x in argv),
            'review bypass/resume argument forbidden')
    overrides = [argv[i + 1] for i, arg in enumerate(argv) if arg == '-c']
    require(set(overrides) == {'web_search="disabled"', 'model_reasoning_effort="xhigh"'} and len(overrides) == 2,
            'review config overrides differ from contract')
    disabled = [argv[i + 1] for i, arg in enumerate(argv) if arg == '--disable']
    require(set(disabled) == REVIEW_DISABLED and len(disabled) == len(REVIEW_DISABLED), 'review feature flags differ')
    probe = out / 'feature-probe.log'
    require(sha(probe) == invocation['feature_probe_sha256'], 'review feature probe drift')
    observed = {line.split()[0]: line.split()[-1] for line in probe.read_text().splitlines() if len(line.split()) >= 3}
    require(all(observed.get(name) == invocation['restricted_features_observed'].get(name) == 'false'
                for name in REVIEW_DISABLED), 'review feature isolation not observed')
    messages, sessions = [], []
    for line in (out / 'thread-reply.log').read_text().splitlines():
        try:
            event = json.loads(line)
        except json.JSONDecodeError:
            continue
        if not isinstance(event, dict):
            continue
        item = event.get('item', {})
        if not isinstance(item, dict):
            item = {}
        require(event.get('type') not in ['hook.started', 'hook.completed']
                and item.get('type') not in ['mcp_tool_call', 'hook_execution'], 'unexpected review connector/hook event')
        if event.get('type') == 'thread.started':
            sessions.append(event.get('thread_id'))
        if event.get('type') == 'item.completed' and item.get('type') in ['agent_message', 'assistant_message']:
            messages.append(item.get('text', ''))
    require(messages and json.loads(messages[-1]) == read(out / 'verdict-returned.json'),
            'returned verdict does not match actual final JSONL agent message')
    return {'invocation_sha256': sha(out / 'invocation.json'), 'thread_reply_sha256': sha(out / 'thread-reply.log'),
            'feature_probe_sha256': sha(probe), 'model_requested': invocation['model_requested'],
            'harness_version_observed': invocation['harness_version_observed'], 'reviewer_sessions_observed': sessions,
            'limits': 'Observed CLI settings/events and unchanged inputs; not cryptographic proof of provider authorship.'}


def latest_verdict(root, subject):
    directory = root / 'docs/planning/reviews' / subject
    key = lambda p: [int(x) if x.isdigit() else x for x in re.findall(r'\d+|[a-z]+', p.name)]
    for attempt in sorted(directory.glob('attempt-*'), key=key, reverse=True):
        for name in ['verdict-returned.json', 'verdict.json']:
            path = attempt / name
            if path.is_file():
                return path, read(path)
    raise ValueError('no actual verdict for ' + subject)


def verify_members(root):
    members = []
    for subject in MEMBERS + [f'P{i:02}' for i in range(1, 8)]:
        if subject == 'C29':
            verify_transition(root)
            members.append(historical_c29(root))
            continue
        if subject == 'C30':
            # Explicit owner completion, not a full-9 numeric verdict or audit.
            # The shared policy validates exact note/verdict/receipt/audit and
            # the original current source scope before returning raw metadata.
            members.append(verify_c30_owner_closure(root))
            continue
        if subject == 'C33':
            decision = root / DECISIONS[subject]
            require(decision.is_file(), 'missing C33 supersession decision')
            members.append({'subject': subject, 'standing': 'superseded under D059; no numeric verdict invented',
                            'owner_decision': DECISIONS[subject], 'owner_decision_sha256': sha(decision)})
            continue
        path, verdict = latest_verdict(root, subject)
        request = read(path.parent / 'request.json')
        require(verdict['subject'] == subject and verdict['request_id'] == request['request_id']
                and verdict['attempt_id'] == request['attempt_id'] and verdict['fingerprints'] == request['fingerprints'],
                subject + ': verdict/request binding mismatch')
        item = {'subject': subject, 'record': str(path.relative_to(root)), 'sha256': sha(path),
                'request': str((path.parent / 'request.json').relative_to(root)),
                'request_sha256': sha(path.parent / 'request.json'), 'scores': verdict['scores'],
                'recommendation': verdict['recommendation']}
        if subject == 'C41':
            decision = root / DECISIONS[subject]
            require(decision.is_file(), 'missing C41 explicit owner override')
            require(verdict['attempt_id'] == 'attempt-09' and verdict['scores'] == dict(
                architecture=9, behavior=8, authority_security=9, validation=8, overall=8),
                'C41 override applies only to its unchanged recorded numeric verdict')
            item.update(owner_decision=DECISIONS[subject], owner_decision_sha256=sha(decision),
                        standing='explicit owner override; actual numeric verdict unchanged')
        else:
            full_gate(verdict)
        receipt = path.parent / 'receipt.json'
        if receipt.is_file():
            value = read(receipt)
            require(value.get('verdict_sha256', value.get('raw_verdict_sha256')) == sha(path), subject + ': receipt verdict hash mismatch')
            require(value.get('scores', value.get('verdict', {}).get('scores')) == verdict['scores'], subject + ': receipt scores mismatch')
            require(value['request_id'] == verdict['request_id'] and value['fingerprints'] == verdict['fingerprints'],
                    subject + ': receipt request/fingerprint mismatch')
            item.update(receipt=str(receipt.relative_to(root)), receipt_sha256=sha(receipt))
        if subject in ['C29', 'C30', 'C31', 'C32']:
            audit = path.parent / 'post-review-audit.json'
            value = read(audit)
            require(value['full_gate_pass'] and value['compiled_inputs_unchanged']
                    and value['raw_verdict_sha256'] == sha(path) and value['request_sha256'] == sha(path.parent / 'request.json'),
                    subject + ': missing/stale full-gate audit')
            # These remedial generations must still cover the current scoped bytes.
            inventory = read(path.parent / 'source-inventory.json')
            for name, digest in inventory['files'].items():
                require(sha(root / name) == digest, subject + ': reviewed source/evidence drift: ' + name)
            item.update(audit=str(audit.relative_to(root)), audit_sha256=sha(audit))
        members.append(item)
    return members


def native_build(root, prepared):
    """The current fixture build's actual before/after compiled-input seal."""
    path = root / 'docs/planning/evidence/P09/native-build-inputs.json'
    build = read(path)
    require(build['schema'] == 'airs-v2/p09-native-build-inputs/1', 'native build-input schema mismatch')
    require(re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]*', build['generation']) is not None,
            'invalid native generation')
    prefix = 'docs/planning/evidence/P09/preparations/' + build['generation'] + '/'
    require(build['prepare'] == 'docs/planning/evidence/P09/native-prepare.json'
            and build['command']['log'] == prefix + 'native-prepare.log'
            and build['command']['cwd'] == str(root)
            and build['command']['argv'] == ['cargo', 'run', '--locked', '-p', 'xtask', '--', 'native-prepare',
                                            '--phase', 'P09', '--target', 'aarch64-apple-darwin', '--features', 'test-fixtures'],
            'native build command/path mismatch')
    require(build['worktree_diff'] == prefix + 'fixture-remediation.diff'
            and sha(root / build['worktree_diff']) == build['worktree_diff_sha256'], 'native source-diff provenance mismatch')
    require(build['prepare_sha256'] == sha(root / build['prepare'])
            and build['executable_sha256'] == prepared['executable_sha256']
            and build['bundle_sha256'] == prepared['bundle_sha256'], 'native build/preparation binding mismatch')
    require(build['command']['exit'] == 0 and sha(root / build['command']['log']) == build['command']['log_sha256'],
            'native build command failed or log changed')
    value = build['inputs']
    require(value['procedure'] == 'docs/planning/evidence/P09/runs/hash-inputs.sh'
            and value['before_inventory'] == prefix + 'inputs-before.sha256'
            and value['after_inventory'] == prefix + 'inputs-after.sha256', 'native inventory paths mismatch')
    procedure = root / value['procedure']
    require(sha(procedure) == value['procedure_sha256'], 'native build inventory procedure drift')
    require(value['before_digest'] == value['after_digest'] == sha(root / value['before_inventory'])
            == sha(root / value['after_inventory']), 'native build inputs changed')
    require(value['unchanged'] is True and value['members'] == len((root / value['before_inventory']).read_text().splitlines())
            and value['members'] > 0, 'native build inventory declaration mismatch')
    with tempfile.TemporaryDirectory(prefix='p09-native-inputs-') as scratch:
        current = subprocess.check_output(['/bin/bash', str(procedure), str(root), str(Path(scratch) / 'inputs.sha256')], text=True).strip()
    require(current == value['before_digest'], 'current compiled inputs differ from prepared native build')
    return {'record': str(path.relative_to(root)), 'sha256': sha(path),
            'generation': build['generation'], 'input_digest': current}


def verify_native(root):
    controller = root / 'docs/planning/evidence/P09/native-controller'
    sys.path.insert(0, str(controller))
    from native_requirements import load_requirements, assess_record
    sys.path.insert(0, str(root / 'tools/native-acceptance'))
    import wizard_coverage
    phase = root / 'docs/planning/evidence/P09'
    prepared = read(phase / 'native-prepare.json')
    require(prepared['subject'] == 'P09' and prepared['features'] == ['test-fixtures']
            and prepared['target'] == 'aarch64-apple-darwin', 'wrong prepared subject/features/target')
    executable = root / prepared['bundle'] / 'Contents/MacOS/prisma-airs-desktop-v2'
    require(sha(executable) == prepared['executable_sha256'], 'prepared executable drift')
    build = native_build(root, prepared)
    records = []
    for subject, required in sorted(load_requirements(root).items()):
        directory = phase if subject == 'C44' else phase / 'native/cumulative' / subject
        path = directory / 'native-acceptance.json'
        record, execution = read(path), read(directory / 'execution.json')
        require(record['subject'] == subject, subject + ': wrong record subject')
        ids = [check['id'] for check in record['checks']]
        require(len(ids) == len(set(ids)), subject + ': duplicate native check IDs')
        require(sha(directory / 'native-prepare.json') == sha(phase / 'native-prepare.json'), subject + ': preparation mismatch')
        errors, carried = assess_record(record, root, prepared['executable_sha256'], required)
        require(not errors, subject + ': required native checks fail: ' + str(errors))
        allowed_carried = {'C24': {'C24-N06'}, 'C31': {'C31-N01', 'C31-N04'}}
        require(set(carried) <= allowed_carried.get(subject, set()), subject + ': unapproved superseded raw check')
        require(execution['driver_return_code'] == 0 or (execution['driver_return_code'] == 1 and bool(carried)),
                subject + ': unexplained driver exit')
        require(execution['schema'] == 'airs-v2/p09-native-execution/1'
                and execution['source_root'] == str(root)
                and execution['driver_subject'] == subject and execution['phase'] == 'P09'
                and execution['executable_sha256'] == prepared['executable_sha256']
                and execution['raw_record_sha256'] == sha(path)
                and execution['required_check_errors'] == errors and execution['superseded_raw_checks'] == carried,
                subject + ': execution binding mismatch')
        for field, source in [('controller_sha256', controller / 'run-p09-native.py'),
                              ('requirements_sha256', controller / 'native_requirements.py'),
                              ('driver_sha256', root / 'tools/native-acceptance' / ('acceptance_' + subject.lower() + '.py'))]:
            require(execution[field] == sha(source), subject + ': execution source drift: ' + field)
        require(subprocess.run(['git', 'merge-base', '--is-ancestor', execution['source_head'], 'HEAD'],
                               cwd=root, capture_output=True).returncode == 0, subject + ': native source is not an integrated ancestor')
        if subject == 'C43':
            errors = wizard_coverage.problems(read(directory / 'native/observations.json')['observations'], root)
            require(not errors, 'C43 retained coverage: ' + str(errors))
        records.append({'subject': subject, 'record': str(path.relative_to(root)), 'sha256': sha(path),
                        'started_utc': execution['started_utc'], 'finished_utc': execution['finished_utc'],
                        'execution': str((directory / 'execution.json').relative_to(root)),
                        'execution_sha256': sha(directory / 'execution.json'),
                        'required_targets': sorted(required), 'superseded_raw_checks': carried,
                        'executable_sha256': prepared['executable_sha256']})
    ordered = sorted(records, key=lambda record: datetime.datetime.fromisoformat(record['started_utc']))
    require([record['subject'] for record in ordered[:2]] == ['C36', 'C43'] and ordered[-1]['subject'] == 'C44',
            'native order requires C36/C43 first and cumulative C44 last')
    for index, record in enumerate(ordered):
        start = datetime.datetime.fromisoformat(record['started_utc'])
        end = datetime.datetime.fromisoformat(record['finished_utc'])
        require(start <= end, 'invalid native execution interval')
        if index:
            require(datetime.datetime.fromisoformat(ordered[index - 1]['finished_utc']) <= start, 'native UI writers overlap')
    return {'prepare': str((phase / 'native-prepare.json').relative_to(root)),
            'prepare_sha256': sha(phase / 'native-prepare.json'), 'build_inputs': build, 'records': records,
            'performance': historical_performance(root), 'source_transition': verify_transition(root)}


def verify_checks(root, record_path):
    run = read(record_path)
    baseline = read(root / 'docs/planning/evidence/P09/runs/run-attempt-02.json')
    require(run['subject'] == 'P09' and set(run['runs']) == set(baseline['runs']) and len(run['runs']) == 18,
            'phase requires the exact 18 P09 checks')
    require(Path(run['cwd']).resolve() == root, 'check run is not from this active clone')
    for name, prefix in [('rustc', 'rustc 1.99.0 '), ('cargo', 'cargo 1.99.0 '), ('node', 'v24.21.0'), ('pnpm', '10.6.5')]:
        require(run['toolchain'][name] == prefix if name in ['node', 'pnpm'] else run['toolchain'][name].startswith(prefix),
                'check toolchain mismatch: ' + name)
    for name, result in run['runs'].items():
        require(result['exit'] == 0 and sha(root / result['log']) == result['log_sha256'], 'failed/stale check: ' + name)
    inputs = run['inputs']
    require(inputs['unchanged'] and inputs['before_digest'] == inputs['after_digest'], 'check input drift')
    procedure = root / 'docs/planning/evidence/P09/runs/hash-inputs.sh'
    require(sha(procedure) == inputs['procedure_sha256'], 'inventory procedure drift')
    flags = ['docs/architecture/' + p for p in ['checks.json', 'native-checks.json', 'toolchain.json', 'budgets.json']]
    flags += ['apps/desktop/src-tauri/tauri.conf.json', 'apps/desktop/src-tauri/tauri.fixture.conf.json']
    flags += sorted(str(p.relative_to(root)) for p in (root / 'apps/desktop/src-tauri/capabilities').glob('*.json'))
    flags += ['rust-toolchain.toml']
    flag_inventory = subprocess.check_output(['/usr/bin/shasum', '-a', '256', *flags], cwd=root)
    require(hashlib.sha256(flag_inventory).hexdigest() == run['build_flags']['digest'], 'current build flags differ from checks')
    directory = record_path.parent / run['attempt']
    for label in ['before', 'after']:
        digest = sha(directory / ('inputs-' + label + '.sha256'))
        require(digest == inputs[label + '_digest'] == inputs[label + '_inventory_sha256'], 'inventory record mismatch')
    audit = read(directory / 'post-check-audit.json')
    require(audit['record_sha256'] == sha(record_path) and audit['check_runs_verified'] == 18
            and audit['current_native_build_inputs_match'] and audit['native_drivers_verified'] == 19,
            'missing/stale check audit')
    with tempfile.TemporaryDirectory(prefix='p09-phase-inputs-') as scratch:
        current = subprocess.check_output(['/bin/bash', str(procedure), str(root), str(Path(scratch) / 'inputs.sha256')], text=True).strip()
    require(current == inputs['before_digest'], 'current compiled inputs differ from checks')
    return run


def verify(root, record_path):
    root = Path(root).resolve()
    run = verify_checks(root, Path(record_path).resolve())
    return {'subject': 'P09', 'check_record_sha256': sha(record_path),
            'input_digest': run['inputs']['before_digest'],
            'native_acceptance': verify_native(root), 'predecessors': verify_members(root),
            'phase_complete': False, 'meaning': 'Inputs qualified for independent review; no phase verdict or completion inferred.'}
