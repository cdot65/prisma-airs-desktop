"""Dated P11 review fixes: affected executions plus explicit unchanged impact.

Original runs/results/candidates remain immutable. This composes evidence; it
never executes tests, changes a verdict, or turns an old process into a new one.
"""
from pathlib import Path
import argparse
import copy
from datetime import datetime
import hashlib
import json
import tarfile

import proof
from standing import ArchivedP11

read, bound, require, sha, digest = proof.read, proof.bound, proof.require, proof.sha, proof.digest
BASELINE = 'docs/planning/evidence/P11/qualification-final-02/qualification.json'
BUILD_HELPER = ('crates/xtask/examples/p11_native_candidate.rs',
                '704087ca7cec096359e17731bea859a7c9bcf814e05856e5819af1f874610062')
# Populated only from the frozen, reviewed P2 recovery patch. Other application
# changes cannot use this one-off impact decision.
APP_DELTA = {'apps/desktop/src/app.tsx': ('9de8de88cb261f3b826d05ab2c2c1ca195a7dc1ba05f68e6bfbb60ad20fdef89',
                              'c3492519fa99eebf5e4bc24bcd603a528d1b0a4ca4c958ebedef8c008714169e'),
 'apps/desktop/src/v2/store.ts': ('e5569371c60bd473fab6f3a944d32b458a7878223e89d883918ca0d4e88d143d',
                                  '8bc2feec3d7de4c6dc8ef31ab6233c88b8209218285917de0c50acd1e70225d8'),
 'apps/desktop/src/v2/workbench/api-client.test.tsx': ('f154cef0337e4f9ded2949958cf995f30df806ea177a984a8e5b9de5f5b5b5c5',
                                                       '161ad953f06049d464ef001857c3cb59fb221e62ff8d9e255c7a75a99d1cb548'),
 'apps/desktop/src/v2/workbench/api-client.tsx': ('c510a3118520f2cbef52f310b70662f0a1cb06e5a42f71a9937088e847000116',
                                                  'b95eb3953ff1e51b16012d38c982b4f71cebc39b12c294b383c2134346d43bde')}
TEST_DELTA = {
    'crates/desktop-core/tests/harness_tools/main.rs': (
        'ad88285ee7ae6b2e3ee1e7828565f50659423fd27483b10f3511eb81a18c287f',
        '7a72bcd6afd6a56edf20b04d1195794b32e0cda85d816f4bc886f99281584861'),
    'crates/desktop-core/tests/ui_v2_inbox/main.rs': (
        '0d19a3058ba5847fdec43860a53fb1454834bb867b398cd89331494ec8d3f443',
        'd746a8ad524e38502daa9dbb1ffb6d4368a4d9324fe4b6fd5ba11b471b4d2811'),
}
SCROLL_FACTS = {'overflowing_fixture_history', 'lower_call_initially_below_viewport',
                'lower_call_reached', 'drawer_content_moved', 'underlying_page_position_unchanged'}


def completed(raw):
    try:
        start = datetime.fromisoformat(raw['started_utc'].replace('Z', '+00:00'))
        end = datetime.fromisoformat(raw['finished_utc'].replace('Z', '+00:00'))
        return raw['exit_code'] == 0 and bool(raw['argv']) and start.tzinfo and end.tzinfo and start <= end
    except (KeyError, TypeError, ValueError):
        return False


def delta(before, after, declared):
    changed = sorted(n for n in set(before) | set(after) if before.get(n) != after.get(n))
    require(changed == declared and APP_DELTA, 'remediation delta is undeclared/unfrozen')
    pairs = APP_DELTA | TEST_DELTA
    for name in changed:
        if name in pairs:
            require(tuple(files.get(name, {}).get('sha256') for files in (before, after)) == pairs[name],
                    'remediation is not the frozen review fix: ' + name)
        elif name == BUILD_HELPER[0]:
            require(name not in before and after[name].get('sha256') == BUILD_HELPER[1],
                    'versioned build helper is not the frozen reviewed adapter')
        else:
            require(name.startswith(('tools/p11/', 'tools/native-acceptance/')) or name in (
                '.gitattributes', 'docs/planning/refinements/2026-11-04/execution.md',
                'crates/xtask/examples/p11_native_candidate.rs'),
                'unrelated production/configuration change: ' + name)
    require(set(pairs) <= set(changed), 'review fix source change missing')
    return changed


def dependencies(files, cid, helper_paths=()):
    """Conservative closure, except the exact hash-pinned remediation paths."""
    roots = ('crates/desktop-core/', 'crates/xtask/src/', 'packages/desktop-contracts/',
             'resources/', 'docs/architecture/', '.cargo/')
    if cid.startswith('C') or '/frontend/' in cid or cid == 'test:C47/fixture':
        roots += ('apps/desktop/',)
    exact = {'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'pnpm-lock.yaml',
             'pnpm-workspace.yaml', 'package.json', 'crates/xtask/Cargo.toml'}
    return sorted(({n for n in files if n in exact or n.startswith(roots)} | set(helper_paths))
                  - set(APP_DELTA) - set(TEST_DELTA))


def preserved_bundle(root, reference, original):
    record = read(bound(root, reference))
    require(record['schema'] == 'airs-desktop/p11-preserved-candidate/1'
            and record['old_qualification']['path'] == BASELINE
            and record['original_prepare'] == original['prepare'], 'preserved candidate source differs')
    archive = bound(root, record['archive'])
    require(archive.stat().st_size == record['archive']['size'], 'preserved archive size differs')
    entries, executable = [], None
    with tarfile.open(archive) as stream:
        members = stream.getmembers()
        prefix = members[0].name.rstrip('/') + '/'
        for item in members:
            require(not item.name.startswith('/') and '..' not in Path(item.name).parts,
                    'preserved archive path escapes application')
            if item.isdir():
                continue
            require(item.name.startswith(prefix) and (item.isfile() or item.issym()),
                    'unsupported preserved application member')
            name = item.name[len(prefix):]
            value = (hashlib.sha256(item.linkname.encode()).hexdigest() if item.issym()
                     else hashlib.sha256(stream.extractfile(item).read()).hexdigest())
            mode = 'symlink' if item.issym() else '100755' if item.mode & 0o111 else '100644'
            entries.append((name, mode, value))
            if name == 'Contents/MacOS/prisma-airs-desktop': executable = value
    complete = hashlib.sha256(''.join(f'{n}\0{m}\0{v}\n' for n, m, v in sorted(entries)).encode()).hexdigest()
    require(complete == record['bundle_sha256'] == original['bundle_sha256']
            and executable == record['executable_sha256'] == original['executable_sha256'],
            'preserved original application bytes differ')


def scroll(root, reference, baseline, original, files):
    supplement = read(bound(root, reference))
    raw = read(bound(root, supplement['raw']))
    execution = read(bound(root, supplement['execution']))
    bound(root, supplement['source'])
    require(supplement['schema'] == 'airs-desktop/p11-drawer-scroll-supplement/1'
            and supplement['baseline_qualification'] == raw['baseline_qualification'] == baseline
            and supplement['assertion'] == 'C47-N02'
            and set(supplement['facts']) == SCROLL_FACTS
            and supplement['facts'] == raw['facts']
            and all(v is True for v in raw['facts'].values()), 'drawer scrolling not actually proved')
    require(completed(raw)
            and raw['fixture_state_unchanged'] and raw['fixture_modes_unchanged']
            and raw['trust_roots_unchanged']
            and raw['state_before_sha256'] == raw['state_after_sha256']
            and raw['source_sha256'] == supplement['source']['sha256'], 'drawer probe provenance differs')
    require(all(supplement[k] == raw[k] == original[k] for k in ('executable_sha256', 'bundle_sha256'))
            and all(files.get(n, {}).get('sha256') == value
                    for n, value in supplement['helper_closure'].items()), 'drawer original artifact/helper differs')
    require(raw['before_anchor']['pos'] == raw['after_anchor']['pos']
            and raw['before_anchor']['size'] == raw['after_anchor']['size']
            and raw['before_first_call']['pos'] != raw['after_first_call']['pos']
            and raw['after_lower_call']['pos'][1] < raw['before_lower_call']['pos'][1],
            'drawer/page measurements do not establish independent scrolling')
    require(len(supplement['captures']) == 2, 'drawer capture pair missing')
    for image in supplement['captures']:
        bound(root, image)
        require(image['visually_reviewed'] is True and image['accepted'] is True and not image['findings'],
                'drawer capture was not individually accepted')
    return execution


def rust_consumers(root, reference, files):
    """The nine formerly unreachable integration consumers, not a Rust suite."""
    manifest = read(bound(root, reference))
    require(manifest['schema'] == 'airs-desktop/p11-oauth-consumer-supplement/1', 'Rust consumer supplement schema')
    filters = {'harness_tools': ['completing_a_tenant_setup_is_the_guarded_switch_path',
        'setup_archive_resolution_save_failure_preserves_pending_work',
        'setup_discard_resolution_save_failure_preserves_pending_work'],
        'ui_v2_inbox': ['setup_warnings_are_inbox_entries_the_operator_can_hold_across_reads',
        'reads_cover_every_chosen_product_with_finite_per_source_outcomes',
        'a_truncated_listing_is_incomplete_coverage_not_a_complete_read',
        'a_failed_product_attempt_is_reported_and_never_replaces_its_last_read',
        'unread_chosen_products_stay_in_coverage_after_a_partial_attempt',
        'a_first_product_recheck_refused_for_a_role_is_a_settled_attempt']}
    targets = {name: len(names) for name, names in filters.items()}
    require(set(manifest['runs']) == set(targets), 'affected Rust consumer target missing')
    for target, count in targets.items():
        item = manifest['runs'][target]
        raw = read(bound(root, {'path': item['record'], 'sha256': item['record_sha256']}))
        log = bound(root, {'path': item['log'], 'sha256': item['log_sha256']})
        require(completed(raw)
                and raw['log_sha256'] == sha(log)
                and raw['argv'] == item['argv'] and '--exact' in raw['argv']
                and raw['argv'][:2] == ['cargo', 'test']
                and raw['argv'][raw['argv'].index('--test')+1] == target
                and raw['argv'][raw['argv'].index('--')+1:] == filters[target]+['--exact', '--nocapture'],
                'affected Rust command failed or differs')
        require(raw['source_files'] and set(TEST_DELTA) <= set(raw['source_files'])
                and all(files.get(n, {}).get('sha256') == value for n, value in raw['source_files'].items()),
                'affected Rust source binding differs')
        require(sum(x['passed'] for x in raw['test_results']) == count
                and all(x['failed'] == x['ignored'] == 0 for x in raw['test_results']),
                'affected Rust consumers were skipped/failed')
    return manifest


def affected_lints(root, receipt, files):
    lint = read(bound(root, receipt['clippy']))
    aliases = {Path(row['public']).suffix: row for row in lint['aliases']}
    for row in lint['aliases']: bound(root, {'path': row['public'], 'sha256': row['sha256']})
    raw = read(root/aliases['.json']['public'])
    require(lint['schema'] == 'airs-p11-affected-clippy-supplement/1' and completed(raw)
            and raw['argv'] == lint['argv'] and '--workspace' not in raw['argv']
            and '--all-targets' not in raw['argv']
            and [raw['argv'][i+1] for i, arg in enumerate(raw['argv']) if arg == '--test']
                == ['harness_tools', 'ui_v2_inbox']
            and raw['log_sha256'] == aliases['.log']['sha256']
            and raw['source_files'] == {n: files[n]['sha256'] for n in TEST_DELTA},
            'changed integration-target Clippy was not passing/source-bound')
    adapter = read(bound(root, receipt['adapter_qa']))
    require(adapter['schema'] == 'airs-p11-versioned-build-adapter-qa/1'
            and (adapter['source']['path'], adapter['source']['sha256']) == BUILD_HELPER
            and adapter['source']['sha256'] == files[adapter['source']['path']]['sha256']
            and set(adapter['runs']) == {'clippy-example-02', 'preflight-test-01'}
            and adapter['preflight_test_counts'] == {'passed': 1, 'failed': 0, 'ignored': 0},
            'versioned build helper QA missing/drifted')
    for item in adapter['runs'].values():
        raw = read(bound(root, {'path': item['record'], 'sha256': item['record_sha256']}))
        log = bound(root, {'path': item['log'], 'sha256': item['log_sha256']})
        require(completed(raw) and raw['argv'] == item['argv'] and raw['log_sha256'] == sha(log)
                and raw['source'] == adapter['source']['path']
                and raw['source_sha256'] == adapter['source']['sha256'], 'actual build helper QA failed/drifted')


def format_selector(root, reference, files, execution_root):
    record = read(bound(root, reference)); bound(root, record['log'])
    names = {n for n in files if not n.startswith('docs/') and (n.endswith('.rs') or Path(n).name in
             ('Cargo.toml', 'rustfmt.toml', '.rustfmt.toml', 'rust-toolchain.toml'))}
    require(record['schema'] == 'airs-desktop/p11-format-selector/1'
            and record['argv'] == ['cargo', 'fmt', '--all', '--', '--check']
            and record['cwd'] == execution_root and record['exit_code'] == 0
            and record['started_utc'] and record['finished_utc']
            and record['source_before'] == record['source_after']
                == {n: files[n]['sha256'] for n in sorted(names)}, 'affected Rust formatting not proved')


def selector_reference(receipt, cid):
    return receipt['recovery'] if cid == 'C48-N03' else receipt['format'] if cid == 'check:C46/check/1' else receipt['source_qa']


def source_transition(root, receipt, candidate, source, files):
    old_record = read(bound(root, receipt['baseline']))
    require(receipt['baseline']['path'] == BASELINE, 'foreign remediation baseline')
    old_coverage = read(bound(root, old_record['coverage']))
    old_candidate = read(bound(root, old_coverage['candidate']))
    before = read(bound(root, old_record['source']))['files']
    delta(before, files, receipt['changed_paths'])
    require(all(candidate[k] == old_candidate[k] for k in ('conditions', 'build_flags'))
            and all(candidate['runner_inputs'][k] == old_record['runner_inputs'][k]
                    for k in ('dependencies', 'build_flags')), 'remediation VM/toolchain/flags changed')
    history = ArchivedP11(root, receipt['historical_context'])
    require(history.source(digest(before)) == root/'docs/planning/evidence/P11/source-history/S3',
            'original reviewed source context differs')
    proof.verify_coverage(root, old_coverage, historical=history)
    preserved_bundle(root, receipt['preserved_candidate'], old_candidate)
    scroll(root, receipt['scroll'], receipt['baseline'], old_candidate, files)
    rust_consumers(root, receipt['rust_consumers'], files)
    affected_lints(root, receipt, files)
    format_selector(root, receipt['format'], files, candidate['execution_root'])
    covered = set()
    require(receipt['helper_qa'], 'remediation affected helper QA missing')
    for ref in receipt['helper_qa']:
        qa = read(bound(root, ref)); bound(root, qa['log'])
        require(qa['schema'] == 'airs-desktop/p11-helper-qa/1' and qa['argv']
                and qa['exit_code'] == 0 and qa['tests']['passed'] > 0 and qa['tests']['failed'] == 0
                and qa.get('source_before') == qa.get('source_after')
                and qa['helper_files'] and all(files.get(n, {}).get('sha256') == value
                    for n, value in qa['helper_files'].items()), 'remediation helper QA failed/drifted')
        covered.update(qa['helper_files'])
    require({n for n in receipt['changed_paths'] if n.endswith('.py')} <= covered,
            'remediation changed helper lacks affected QA')
    return old_record, old_coverage, history


def source_qa(root, reference, files):
    supplement = read(bound(root, reference))
    require(supplement['schema'] == 'airs-desktop/p11-c48-source-supplement/1'
            and supplement['source_files'] == {n: files[n]['sha256'] for n in APP_DELTA},
            'C48 affected source inputs differ')
    producers = [read(bound(root, ref)) for ref in supplement['producer_receipts']]
    originals = [row for producer in producers for key in ('final_checks', 'retained_prior_focused_checks')
                 for row in producer.get(key, [])]
    expected = {'C48/recovery-final02': 4, 'C48/typecheck-final02': None,
        'C48/controllers-final': 20, 'C48/api-prior': 42, 'C48/shell-and-cases-prior': 45}
    checks = supplement['checks']
    require(len(checks) == len(expected) and {c['id'] for c in checks} == set(expected),
            'C48 selected recovery/cross-consumer coverage missing')
    earlier = {
        'apps/desktop/src/v2/workbench/api-client.tsx': {
            '50b881c165ce183abe845f4c572d687fe7bc4b0041a1a69c875394c55a67bb1d',
            '0d1da0ed31d0ff86e4d46fb0392f08ad06945b30eb47c7cf97c114fba46e63d3'},
        'apps/desktop/src/v2/workbench/api-client.test.tsx': {
            '2d07b3ec2c0d340b30c4cf23877c05b548d30f34c7a4a98fe2dc72433f2f01eb',
            '7a68e497d5056c30ebb8aa1f9f9c8216d7a28e979d86496ea74cbce9c255de03'}}
    for item in checks:
        require(item['exit_code'] == 0 and item.get('passed') == expected[item['id']]
                and item['argv'] and item['cwd'], 'C48 selected command failed')
        log = bound(root, item['log'])
        require(any(all(old.get(k) == item.get(k) for k in ('argv', 'cwd', 'exit_code', 'passed'))
                    and old['log_sha256'] == sha(log) for old in originals),
                'C48 selection is not an original actual execution')
        require(item['source_hashes'] and set(item['source_inputs']) == set(item['source_hashes']),
                'C48 original source scope missing')
        for name, value in item['source_hashes'].items():
            require(sha(bound(root, item['source_inputs'][name])) == value
                    and (files.get(name, {}).get('sha256') == value or value in earlier.get(name, set())),
                    'C48 selected source delta is not the bounded recovery refinement')
        if item['id'].endswith('final02'):
            require(item['source_hashes'] == supplement['source_files'], 'final selector source was retagged')
    return supplement


def recovery(root, reference, baseline, candidate, old_result, files):
    supplement = read(bound(root, reference))
    raw = read(bound(root, supplement['raw']))
    bound(root, supplement['execution']); bound(root, supplement['source'])
    needed = {'missing_restore_warning_visible', 'recovery_survives_home_back', 'retry_is_explicit',
              'retry_recovers_when_root_restored', 'intentional_close_stays_closed'}
    require(supplement['schema'] == 'airs-desktop/p11-recovery-supplement/1'
            and supplement['baseline_qualification'] == baseline
            and set(supplement['facts']) == needed
            and supplement['facts'] == raw['facts'] and all(v is True for v in raw['facts'].values()),
            'affected recovery observations incomplete')
    require(completed(raw) and raw['cwd'] == candidate['execution_root']
            and supplement['prepare'] == candidate['prepare']
            and raw['source_sha256'] == supplement['source']['sha256']
            and all(supplement[k] == raw[k] == candidate[k] for k in ('executable_sha256', 'bundle_sha256'))
            and raw['conditions'] == candidate['conditions'], 'recovery execution/candidate differs')
    require(supplement['helper_closure'] == raw.get('helper_closure') and supplement['helper_closure']
            and all(files.get(n, {}).get('sha256') == value
            for n, value in supplement['helper_closure'].items()), 'recovery helper inputs changed')
    require(supplement['retained_result'] == old_result and supplement['retained_reason'],
            'old N03 first-use/save observations were lost or retagged')
    require(supplement['captures'], 'recovery pixels missing')
    for image in supplement['captures']:
        bound(root, image)
        require(image['visually_reviewed'] is True and image['accepted'] is True and not image['findings'],
                'recovery capture not individually accepted')
    return supplement


SOURCE_IDS = {'check:C46/check/1', 'check:C46/check/4', 'test:C47/fixture', 'test:C48/frontend/1', 'test:C48/frontend/2'}


def verify_coverage(root, qualified, historical=None):
    receipt = read(bound(root, qualified['review_remediation']))
    candidate = read(bound(root, qualified['candidate']))
    source = historical.source(qualified['source_inventory_sha256']) if historical else root
    files = read(bound(root, receipt['current_source']))['files']
    require(digest(files) == qualified['source_inventory_sha256']
            and files == ({str(p.relative_to(source)): proof.file_identity(p) for p in source.rglob('*')
                           if p.is_file() or p.is_symlink()} if historical else proof.source_inventory(root)),
            'remediation current source differs')
    require(receipt['schema'] == 'airs-desktop/p11-review-remediation/1'
            and receipt['runner_inputs'] == candidate['runner_inputs'], 'remediation source binding missing')
    original_record, original_coverage, history = source_transition(root, receipt, candidate, source, files)
    source_qa(root, receipt['source_qa'], files)
    originals = {r['id']: r for r in original_coverage['coverage']}
    recovery(root, receipt['recovery'], receipt['baseline'], candidate, originals['C48-N03']['result'], files)
    modes = {cid: 'executed' if cid in SOURCE_IDS | {'C48-N03'} else 'carried_unaffected' for cid in originals}
    plan = read(bound(root, qualified['plan']))
    proof.verify_plan(source, plan, modes)
    rows = qualified['coverage']
    require(len(rows) == len(originals) and {r['id'] for r in rows} == set(originals), 'remediation coverage missing/duplicated')
    require(set(receipt['unaffected']) == set(originals)-SOURCE_IDS-{'C48-N03'}, 'unchanged-impact reason missing')
    requirements = {r['id']: r for r in plan['requirements']}
    for row in rows:
        cid = row['id']; require(row['disposition'] == modes[cid], 'remediation disposition differs')
        if cid in SOURCE_IDS | {'C48-N03'}:
            expected = {'schema': 'airs-desktop/p11-affected-selector-result/1', 'required_id': cid,
                'actual_evidence': selector_reference(receipt, cid),
                'original_result': originals[cid]['result'],
                'meaning': 'Actual affected selectors supplement the retained original registered goal. No full original command or unrelated native flow is claimed newly executed.'}
            require(read(bound(root, row['result'])) == expected, 'affected goal lacks actual selector evidence')
            continue
        require(row['result'] == originals[cid]['result'] and receipt['unaffected'][cid], 'original result was relabelled')
        original = read(bound(root, row['result'])); origin = read(bound(root, original['candidate']))
        deps = dependencies(files, cid, requirements[cid].get('helper_paths', []))
        require(requirements[cid].get('dependencies') == deps
                and row['impact']['dependencies'] == deps
                and row['impact']['reason'] == receipt['unaffected'][cid]
                and row['impact']['original_candidate_sha256'] == original['candidate']['sha256'],
                'remediation dependency scope/rationale differs')
        impact = read(bound(root, row['impact']['proof']))
        bindings = impact['file_bindings']
        require(set(bindings) == set(deps) and deps
                and impact['unchanged_dependencies'] == deps
                and impact['current_source_inventory_sha256'] == qualified['source_inventory_sha256']
                and impact['original_candidate_sha256'] == original['candidate']['sha256'],
                'remediation dependency proof binding differs')
        old_source = history.source(origin['source_inventory_sha256'])
        for name in deps:
            value = bindings[name]
            require(value['original_sha256'] == value['current_sha256'] == sha(source/name) == sha(old_source/name),
                    'carried dependency changed: ' + name)
    # The old custom partial Check rows were already validated above in their
    # archived source. Their failed full-run status must not be rewritten to
    # satisfy the generic all-pass-run parser. Other original raw rows use it.
    generic_ids = {cid for cid in originals if cid not in SOURCE_IDS | {'C48-N03'} and not cid.startswith('check:')}
    runtime_plan = read(bound(root, qualified['runtime_plan']))
    require(runtime_plan == dict(plan, requirements=[r for r in plan['requirements'] if r['id'] in generic_ids]),
            'remediation generic coverage projection differs')
    proof.coverage.verify(root, dict(qualified, plan=qualified['runtime_plan'],
        coverage=[r for r in rows if r['id'] in generic_ids]), historical=historical)
    return {'qualified': True, 'required': len(rows),
            'dispositions': {m: sum(r['disposition'] == m for r in rows) for m in sorted(set(modes.values()))},
            'candidate_sha256': qualified['candidate']['sha256'], 'source_inventory_sha256': qualified['source_inventory_sha256']}


def versioned_build(root, build, candidate):
    path = candidate['prepare']['path']
    prefix, suffix = 'docs/planning/evidence/P11/native-build-', '-vm/native-prepare.json'
    number = path.removeprefix(prefix).removesuffix(suffix)
    require(number.isdigit() and int(number) >= 5 and path == f'{prefix}{int(number):02}{suffix}',
            'new build may not overwrite the original canonical preparation')
    argv = ['cargo', 'run', '--locked', '-p', 'xtask', '--example', 'p11_native_candidate', '--', '--out', path]
    commands = [step['argv'] for step in build['steps']]
    require(commands and commands[-1] in (argv, argv[:2]+['--offline']+argv[2:])
            and commands[:-1] in ([], [['pnpm', '--filter', '@prisma-airs/desktop-contracts', 'typecheck'],
                ['pnpm', '--filter', '@prisma-airs/desktop', 'typecheck']])
            and build['application_inputs_after'] == build['application_inputs_before'],
            'versioned native build command/input recipe differs')
    old = read(root/'docs/planning/evidence/P11/native-prepare.json')
    current = read(bound(root, candidate['prepare']))
    require(current['commands'] == old['commands'] and current['target'] == old['target']
            and current['features'] == old['features'] and current['subject'] == 'P11',
            'versioned build changed the fixture recipe')


def make(root, receipt_path, out):
    require(not out.exists(), 'preserve previous remediation qualification')
    receipt = read(receipt_path)
    files = read(bound(root, receipt['current_source']))['files']
    old = read(bound(root, receipt['baseline']))
    original = read(bound(root, old['coverage']))
    prepared = read(bound(root, receipt['prepare']))
    native = read(bound(root, receipt['recovery']))
    candidate = dict(source_inventory_sha256=digest(files), execution_root=str(root),
        runner_inputs=receipt['runner_inputs'], dependency_fingerprint=receipt['runner_inputs']['dependencies'],
        prepare=receipt['prepare'], executable_sha256=prepared['executable_sha256'],
        bundle_sha256=prepared['bundle_sha256'], build_flags={'target': prepared['target'], 'features': prepared['features']},
        conditions=read(bound(root, native['raw']))['conditions'], helper_closure=native['helper_closure'])
    history = ArchivedP11(root, receipt['historical_context'])
    out.mkdir(parents=True)
    def write(name, value):
        p = out/name; p.parent.mkdir(parents=True, exist_ok=True)
        with p.open('x') as stream: json.dump(value, stream, indent=2); stream.write('\n')
        return proof.reference(root, p)
    candidate_ref = write('candidate.json', candidate)
    requirements = copy.deepcopy(read(bound(root, original['plan']))['requirements'])
    by_id = {r['id']: r for r in requirements}; rows = []
    for original_row in original['coverage']:
        cid = original_row['id']; needed = by_id[cid]
        affected = cid in SOURCE_IDS | {'C48-N03'}
        needed.update(affected=affected, must_execute=affected)
        if affected:
            result = write('results/'+cid.replace(':', '-').replace('/', '-')+'.json', {
                'schema': 'airs-desktop/p11-affected-selector-result/1', 'required_id': cid,
                'actual_evidence': selector_reference(receipt, cid),
                'original_result': original_row['result'],
                'meaning': 'Actual affected selectors supplement the retained original registered goal. No full original command or unrelated native flow is claimed newly executed.'})
            rows.append(dict(id=cid, disposition='executed', result=result))
            continue
        result = read(bound(root, original_row['result'])); origin = read(bound(root, result['candidate']))
        deps = dependencies(files, cid, needed.get('helper_paths', [])); needed['dependencies'] = deps
        old_source = history.source(origin['source_inventory_sha256'])
        bindings = {name: {'original_sha256': sha(old_source/name), 'current_sha256': sha(root/name),
            'original_file': proof.reference(root, old_source/name)} for name in deps}
        impact = write('impact/'+cid.replace(':', '-').replace('/', '-')+'.json', {
            'file_bindings': bindings, 'unchanged_dependencies': deps,
            'current_source_inventory_sha256': digest(files), 'original_candidate_sha256': result['candidate']['sha256']})
        rows.append(dict(id=cid, disposition='carried_unaffected', result=original_row['result'], impact={
            'reason': receipt['unaffected'][cid], 'dependencies': deps, 'proof': impact,
            'original_candidate_sha256': result['candidate']['sha256']}))
    plan = {'schema': 'airs-desktop/required-coverage/1', 'subject': 'P11', 'requirements': requirements}
    plan_ref = write('required-coverage.json', plan)
    generic_ids = {r['id'] for r in rows if r['disposition'] == 'carried_unaffected' and not r['id'].startswith('check:')}
    runtime = write('runtime-required-coverage.json', dict(plan,
        requirements=[r for r in requirements if r['id'] in generic_ids]))
    qualified = write('coverage.json', {'schema': 'airs-desktop/qualified-coverage/1', 'plan': plan_ref,
        'candidate': candidate_ref, 'source_inventory_sha256': digest(files), 'coverage': rows,
        'runtime_plan': runtime, 'review_remediation': proof.reference(root, receipt_path)})
    record = {'schema': 'airs-desktop/p11-shared-qualification/1', 'source': receipt['current_source'],
        'coverage': qualified, 'runner_inputs': receipt['runner_inputs'], 'fingerprint': receipt['fingerprint'],
        'native_execution': native['raw'], 'build': receipt['build'], 'runs': old['runs'],
        'historical_baseline': old['historical_baseline'], 'historical_standing': old['historical_standing'],
        'review_remediation': proof.reference(root, receipt_path), 'controller_complete': False,
        'historical_use': 'Original qualification02 results/artifacts remain bound. Only explicit affected source selectors and native recovery execute anew; unrelated results retain original process/source/artifact provenance.'}
    path = write('qualification.json', record)
    return proof.verify(root, root/path['path'])


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('receipt', type=Path)
    parser.add_argument('--out', required=True, type=Path)
    args = parser.parse_args()
    print(json.dumps(make(Path.cwd().resolve(), args.receipt.resolve(), args.out.resolve()), indent=2))
