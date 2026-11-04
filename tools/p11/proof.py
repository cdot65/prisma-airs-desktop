"""P11's shared candidate proof. Reuses the existing strict raw-evidence consumer.

One actual phase run remains a phase run when four chunk reviews consume it.
This module neither executes tests nor assigns scores or ledger completions.
"""
from pathlib import Path
import json
import hashlib
import importlib.util
import os
import platform
import stat
import subprocess
import sys
import argparse

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'p10'))
from candidate import bundle_hash, helper_closure, source_inventory, file_identity, digest
import qualification as coverage

CHUNKS = ('C46', 'C47', 'C48', 'C49')
APPLICATION_INPUTS = ('apps/desktop', 'crates/desktop-core', 'packages/desktop-contracts',
                      'resources', 'Cargo.toml', 'Cargo.lock', 'pnpm-lock.yaml',
                      'pnpm-workspace.yaml', 'package.json', 'rust-toolchain.toml',
                      'docs/architecture/inputs.lock.json')
require, read, sha, bound = coverage.require, coverage.read, coverage.sha, coverage.bound
CHECK_CARRY_PATHS = {'crates/desktop-core/src/authority/setup_warning_domain.rs',
                     'crates/desktop-core/tests/setup_warning_recheck.rs',
                     'tools/p11/proof.py', 'tools/p11/test_proof.py',
                     'tools/p11/standing.py', 'tools/p11/README.md',
                     'docs/planning/refinements/2026-11-04/execution.md'}
CHECK_LINT_SHA256 = ('78670431d22db13786eccddb0fb30328d0596378937d3cc5dacefb91d6089c5a',
                    '53416c79363d279943b51f7f935514be01b576166241ec3bb2af24a8e8e34e27')
CHECK_TEST_PATH = 'crates/desktop-core/tests/setup_warning_recheck.rs'
CHECK_TEST_LINT_SHA256 = ('e9133b4869104afc448a5701685b4b72dffd1405ed687284f370cec04fa906f5',
                         'e4aef28bf18b15f77d3fac003fbd65c2b6379cd97ee58e8ed43fda1a1f5fc7e5')
CHECK_CARRY_LABELS = {'setup/1', 'setup/2', 'C46/check/2', 'C46/check/3',
                     'C46/check/4', 'C46/check/5'}


def xtask_source(root):
    """Recompute fingerprint.rs source inventory/hash, including deleted files.

    This joins the current Python review seal to the Rust runner's actual source
    binding. A stale fingerprint JSON cannot bind new source to old test results.
    """
    names = subprocess.check_output(['/usr/bin/git', 'ls-files', '-z', '--cached', '--others',
                                     '--exclude-standard'], cwd=root).decode().split('\0')
    names = sorted({name for name in names if name and name != 'docs/planning/status.md'
                    and not name.startswith(('docs/planning/reviews/', 'docs/planning/evidence/'))})
    parts = []
    for name in names + ['.git/config']:
        path = root / name
        try:
            metadata = path.lstat()
        except OSError:
            mode, value = 'deleted', ''
        else:
            if path.is_symlink():
                mode = 'symlink'
                value = hashlib.sha256(os.fsencode(os.readlink(path))).hexdigest()
            else:
                mode = '100755' if stat.S_IMODE(metadata.st_mode) & 0o111 else '100644'
                value = sha(path)
        parts.append(f'{name}\0{mode}\0{value}\n')
    return hashlib.sha256(''.join(parts).encode()).hexdigest()


def application_files(root):
    return set(subprocess.check_output(['/usr/bin/git', 'ls-files', '-z', '--cached', '--others',
                                       '--exclude-standard', *APPLICATION_INPUTS],
                                       cwd=root).decode().strip('\0').split('\0'))


def current_fingerprints(root):
    """Metadata audit only: no tests, native flow, frontend or application build."""
    env = {key: os.environ[key] for key in ('HOME', 'USER', 'LOGNAME', 'TMPDIR', 'LANG', 'LC_ALL')
           if key in os.environ}
    env['PATH'] = ':'.join([str(root / '.tools/bin'), str(Path.home() / '.cargo/bin'),
                            str(Path.home() / '.nvm/versions/node/v24.21.0/bin'),
                            '/usr/bin', '/bin', '/usr/sbin', '/sbin'])
    env['RUSTUP_TOOLCHAIN'] = '1.99.0'
    result = subprocess.run([str(Path.home() / '.cargo/bin/cargo'), 'run', '--locked', '-p',
                             'xtask', '--', 'fingerprint', '--phase', 'P11'], cwd=root,
                            env=env, capture_output=True, text=True)
    require(result.returncode == 0, 'current metadata fingerprint audit failed: ' + result.stderr[-2000:])
    return json.loads(result.stdout)


def native_origins(root, acceptance, execution, candidate):
    spec = importlib.util.spec_from_file_location('p11_native_proof', Path(__file__).with_name('native.py'))
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module.validate(root, acceptance, execution, candidate)



def native_driver(root, performed):
    raw = (read(bound(root, performed['origins'][-1]['observations']))
           if performed.get('schema') == 'airs-desktop/p11-native-selection/1' else performed)
    argv = raw.get('argv', [])
    names = ('acceptance_p11', 'acceptance_p11_resume', 'acceptance_p11_continue')
    require(len(argv) > 1 and argv[1] in ['tools/native-acceptance/' + name + '.py' for name in names],
            'unknown actual native driver')
    return Path(argv[1]).stem


def equivalent_native_origin(root, raw_path, raw, candidate, source):
    """Original helper bytes verified in a current source context, not a new run."""
    require(raw['executable_sha256'] == candidate['executable_sha256']
            and raw['bundle_sha256'] == candidate['bundle_sha256']
            and raw['host'] == candidate['conditions']['platform'],
            'native origin application or execution platform changed')
    require(raw['helper_closure'] and all((source / name).resolve().is_relative_to(source.resolve())
            and (source / name).is_file() and sha(source / name) == value
            for name, value in raw['helper_closure'].items()),
            'original native helper bytes changed')
    return {key: candidate[key] for key in
        ('source_inventory_sha256', 'execution_root', 'prepare', *coverage.EXACT_FIELDS)} | {
        'helper_closure': raw['helper_closure'],
        'native_observation': reference(root, raw_path),
        'meaning': 'Original native process observations retain their actual arguments, times and exit status. '
                   'The source inventory identifies only the current exact-helper-byte equivalence context; '
                   'the original observations recorded no whole-source fingerprint. No current execution '
                   'or original global source fingerprint is claimed.'}


def validate_execution(root, performed, candidate, acceptance=None, execution=None):
    if performed.get('schema') == 'airs-desktop/p11-native-selection/1':
        require(acceptance is not None and execution is not None, 'native continuation paths missing')
        return native_origins(root, acceptance, execution, candidate)
    require(performed['status'] == 'observed' and performed['exit_code'] == 0,
            'native observation process failed or did not finish')
    require(Path(performed['argv'][0]).name in ('python3', 'Python3', 'Python')
            and performed['argv'][1] == 'tools/native-acceptance/acceptance_p11.py',
            'native execution entrypoint differs')
    require(performed['executable_sha256'] == candidate['executable_sha256']
            and performed['bundle_sha256'] == candidate['bundle_sha256'],
            'native observation candidate differs')
    require(performed['helper_closure'] == candidate['helper_closure'], 'native helper drift')


def frontend_test_carry(root, record, build_path, build, candidate, changed):
    """A test-only edit may retain the native build after an exact frontend rebuild.

    The native executable/bundle are verified independently. Original build
    outcomes stay bound; this does not claim a newly executed native build.
    """
    require(changed and all(name.startswith('apps/desktop/src/')
                           and name.endswith(('.test.ts', '.test.tsx')) for name in changed),
            'application-affecting build input changed')
    carry = read(bound(root, record['build_input_carry']))
    require(carry['schema'] == 'airs-desktop/frontend-test-build-carry/1'
            and carry['build'] == reference(root, build_path)
            and carry['prepare'] == candidate['prepare'] and carry['native_rebuilt'] is False,
            'test-only native build carry is not bound')
    expected = {name: {'before': build['application_inputs_before'][name], 'after': sha(root / name)}
                for name in changed}
    require(carry['changed_inputs'] == expected and carry['reason'], 'test-only delta differs')
    equivalence_path = bound(root, carry['frontend_equivalence'])
    equivalence = read(equivalence_path)
    require(equivalence['schema'] == 'airs-desktop/frontend-build-equivalence/1'
            and equivalence['cwd'] == str(root)
            and len(equivalence['argv']) == 8
            and equivalence['argv'][:7] == ['pnpm', '--filter', '@prisma-airs/desktop',
                                           'exec', 'vite', 'build', '--outDir']
            and Path(equivalence['argv'][7]).resolve().is_relative_to(root / '.evidence-private'),
            'frontend equivalence command differs')
    current_inputs = {name: sha(root / name) for name in sorted(application_files(root))}
    require(equivalence['application_inputs_before'] == current_inputs
            and equivalence['application_inputs_after'] == current_inputs,
            'frontend equivalence inputs changed or were not bound')
    require(equivalence['exit_code'] == 0 and equivalence['frontend_byte_identical'] is True
            and equivalence['original_frontend_unchanged'] is True
            and equivalence['original_prepare_sha256'] == candidate['prepare']['sha256'],
            'frontend rebuild equivalence failed or names another candidate')
    original = equivalence['original_frontend_files']
    require(original and original == equivalence['rebuilt_frontend_files'],
            'frontend rebuilt bytes or membership differ')
    current = {str(path.relative_to(root / 'apps/desktop/dist')): sha(path)
               for path in sorted((root / 'apps/desktop/dist').rglob('*')) if path.is_file()}
    require(current == original, 'retained frontend bytes changed after equivalence proof')
    require(sha(equivalence_path.parent / 'build.log') == equivalence['log_sha256'],
            'frontend equivalence build log drift')


def rust_test_carry(root, record, build_path, build, candidate, changed):
    """Retain the native build for the exact dated unwrap-to-expect test edit.

    Current Check/Test evidence is validated before this consumer. Rust test
    source is not part of the application target; no native build is claimed.
    """
    require(changed == [CHECK_TEST_PATH]
            and (build['application_inputs_before'].get(CHECK_TEST_PATH),
                 sha(root / CHECK_TEST_PATH)) == CHECK_TEST_LINT_SHA256,
            'build carry is not the reviewed Rust test-only change')
    carry = read(bound(root, record['build_input_carry']))
    require(carry['schema'] == 'airs-desktop/rust-test-build-carry/1'
            and carry['build'] == reference(root, build_path)
            and carry['prepare'] == candidate['prepare'] and carry['native_rebuilt'] is False,
            'Rust test-only native build carry is not bound')
    expected = {CHECK_TEST_PATH: {'before': CHECK_TEST_LINT_SHA256[0],
                                  'after': CHECK_TEST_LINT_SHA256[1]}}
    require(carry['changed_inputs'] == expected and carry['reason'], 'Rust test-only delta differs')
    runs = record.get('runs', {})
    require(runs.get('check') and runs.get('test')
            and carry.get('check_selection') == runs['check']
            and carry.get('tests') == runs['test'], 'Rust test carry lacks current Check/Test bindings')
    bound(root, runs['check'])
    bound(root, runs['test'])


def reference(root, path):
    path = Path(path).resolve()
    require(path.is_relative_to(root.resolve()), 'proof path escapes checkout')
    return {'path': str(path.relative_to(root)), 'sha256': sha(path)}


def command_plan(source, kind):
    manifest = read(source / 'docs/architecture/checks.json')
    require(manifest['phases']['P11'] == list(CHUNKS), 'P11 membership changed')
    rows = [{'label': f'setup/{i+1}', 'argv': argv, 'expects_tests': False}
            for i, argv in enumerate(manifest['setup_commands'])]
    selected = []
    for chunk in CHUNKS:
        entry = manifest['chunks'][chunk]
        if kind == 'check':
            require(entry.get('check_commands'), 'missing explicit P11 check selection')
            commands = [(f'{chunk}/check/{i+1}', argv) for i, argv in enumerate(entry['check_commands'])]
        else:
            commands = [(f'{chunk}/fixture', entry['fixture_command'])]
            commands += [(f'{chunk}/{category}/{i+1}', argv)
                         for category in ('host', 'frontend')
                         for i, argv in enumerate(entry.get(category + '_commands', []))]
        for label, argv in commands:
            if any(row['argv'] == argv for row in selected):
                continue
            selected.append({'label': label, 'argv': argv,
                             'expects_tests': 'vitest' in argv or argv[:2] == ['cargo', 'test']})
    return rows + selected


def validate_run(root, path, kind, runner_inputs, labels=None, source=None):
    raw = read(path)
    if raw.get('schema') == 'airs-desktop/p11-check-selection/1':
        require(kind == 'check', 'check selection cannot replace a test run')
        return validate_check_selection(root, path, runner_inputs)
    require(raw['schema'] == 'airs-v2/run/1' and raw['subject'] == 'P11'
            and raw['kind'] == kind, 'not an actual P11 run of the required kind')
    require(raw['status'] == 'passed' and raw['failure'] is None,
            'P11 run is not passing')
    require(raw['inputs_before'] == raw['inputs_after'] == runner_inputs,
            'P11 run inputs differ or changed')
    expected = command_plan(source or root, kind)
    if labels is not None:
        expected = [row for row in expected if row['label'] in labels]
        require(labels and [row['label'] for row in expected] == labels
                and not any(label.startswith('setup/') for label in labels),
                'unknown, duplicate or out-of-order selected command')
    require([row['argv'] for row in raw['commands']] == [row['argv'] for row in expected],
            'P11 command membership/order differs from approved focused plan')
    for actual, needed in zip(raw['commands'], expected):
        require(actual['label'] == needed['label'], 'P11 command label differs')
        require(actual['status'] == 'passed' and actual['exit_code'] == 0
                and not actual['note'], 'P11 command failed or was skipped')
        require(actual['cwd'] in ('.', str(root)), 'P11 execution root differs')
        log = (path.parent / actual['log']).resolve()
        require(log.is_relative_to(root.resolve()) and sha(log) == actual['output_sha256'],
                'P11 raw command log drift')
        if needed['expects_tests']:
            require(actual['tests'] and actual['tests']['passed'] > 0
                    and actual['tests']['failed'] == 0, 'P11 target executed no passing tests')
    return raw


def inventory_source(record):
    """Join a retained file inventory to the Rust runner's source fingerprint."""
    parts = []
    for name, identity in sorted(record['files'].items()):
        if name == 'docs/planning/status.md' or name.startswith(('docs/planning/evidence/', 'docs/planning/reviews/')):
            continue
        mode = 'symlink' if identity['kind'] == 'symlink' else ('100755' if identity['mode'] & 0o111 else '100644')
        value = (hashlib.sha256(os.fsencode(identity['target'])).hexdigest()
                 if mode == 'symlink' else identity['sha256'])
        parts.append(f'{name}\0{mode}\0{value}\n')
    config = record['git_config']
    mode = 'deleted' if config is None else ('100755' if config['mode'] & 0o111 else '100644')
    parts.append(f'.git/config\0{mode}\0{config["sha256"] if config else ""}\n')
    return hashlib.sha256(''.join(parts).encode()).hexdigest()


def validate_check_selection(root, path, runner_inputs, source=None, files=None):
    """Account for passing commands of a failed run without relabelling that run.

    Only the two conditional style fixes and this verifier may differ. Current
    Clippy recompiles the affected library; prior bindings/typechecks and setup
    keep their original source, command, timestamps, status and log identities.
    """
    selection = read(path)
    require(selection['schema'] == 'airs-desktop/p11-check-selection/1'
            and selection['runner_inputs'] == runner_inputs and selection['reason'],
            'check selection binding missing')
    before = read(bound(root, selection['original_source']))
    after = read(bound(root, selection['current_source']))
    require(after['files'] == (source_inventory(root) if files is None else files),
            'check selection current source differs')
    if files is None:
        require(selection['execution_root'] == str(root), 'check selection execution root differs')
        config = root / '.git/config'
        require(after['git_config'] == (file_identity(config) if config.is_file() else None),
                'check selection git configuration differs')
    changed = sorted(name for name in set(before['files']) | set(after['files'])
                     if before['files'].get(name) != after['files'].get(name))
    require(changed and changed == selection['changed_paths'] and set(changed) <= CHECK_CARRY_PATHS,
            'undeclared or behavior-affecting check carry delta')
    module = 'crates/desktop-core/src/authority/setup_warning_domain.rs'
    require(tuple(record['files'].get(module, {}).get('sha256') for record in (before, after))
            == CHECK_LINT_SHA256, 'check carry is not the reviewed conditional style change')
    if CHECK_TEST_PATH in changed:
        require(tuple(record['files'].get(CHECK_TEST_PATH, {}).get('sha256') for record in (before, after))
                == CHECK_TEST_LINT_SHA256, 'check carry is not the reviewed Rust test-only change')
    original_path = bound(root, selection['original_run'])
    original = read(original_path)
    supplement_path = bound(root, selection['supplement'])
    supplement = read(supplement_path)
    require(original['schema'] == 'airs-v2/run/1' and original['subject'] == 'P11'
            and original['kind'] == 'check' and original['status'] == 'failed'
            and original['failure'] and original['inputs_before'] == original['inputs_after'],
            'original failed check run missing or inputs drifted')
    require(inventory_source(before) == original['inputs_before']['source']
            and inventory_source(after) == runner_inputs['source'], 'check carry source fingerprint differs')
    require(all(original['inputs_before'][key] == runner_inputs[key]
                for key in ('dependencies', 'build_flags')), 'check carry dependencies or flags differ')
    require(supplement['schema'] == 'airs-desktop/p11-check-supplement/1'
            and supplement['subject'] == 'P11' and supplement['kind'] == 'check'
            and supplement['status'] == 'passed' and supplement['failure'] is None
            and supplement['inputs_before'] == supplement['inputs_after'] == runner_inputs,
            'current check supplement failed or inputs drifted')
    expected = command_plan(source or root, 'check')
    require([row['label'] for row in selection['rows']] == [row['label'] for row in expected]
            and [(row['label'], row['argv']) for row in original['commands']]
            == [(row['label'], row['argv']) for row in expected], 'check selection membership/order differs')
    fresh = [needed for needed, row in zip(expected, selection['rows']) if row['disposition'] == 'executed']
    require([(row['label'], row['argv']) for row in supplement['commands']]
            == [(row['label'], row['argv']) for row in fresh], 'check supplement membership/order differs')
    selected = []
    for needed, row in zip(expected, selection['rows']):
        mode = row['disposition']
        require(mode in ('executed', 'carried_unaffected'), 'unsupported check selection disposition')
        require(mode == 'executed' or (needed['label'] in CHECK_CARRY_LABELS and row.get('reason')),
                'affected check cannot be carried or lacks rationale')
        raw_path, raw = ((supplement_path, supplement) if mode == 'executed' else (original_path, original))
        commands = [item for item in raw['commands'] if item['label'] == needed['label']]
        require(len(commands) == 1, 'selected raw check command missing/duplicated')
        actual = commands[0]
        require(actual['status'] == 'passed' and actual['exit_code'] == 0 and not actual['note']
                and actual['argv'] == needed['argv'] and actual['cwd'] in ('.', selection['execution_root']),
                'selected raw check command failed or differs')
        log = (raw_path.parent / actual['log']).resolve()
        require(log.is_relative_to(root.resolve()) and sha(log) == actual['output_sha256'],
                'selected raw check log drift')
        require(raw.get('run_id') and raw.get('started_utc') and raw.get('finished_utc'),
                'selected raw check provenance missing')
        selected.append({'needed': needed, 'actual': actual, 'raw': raw, 'path': raw_path,
                         'disposition': mode, 'reason': row.get('reason')})
    return {'selection': selection, 'selected': selected}



D2_SOURCE = Path('docs/planning/evidence/P11/source-history/D2')
IMPACT_CHECKS = ('C46/check/1', 'C46/check/4')


def carry_dependencies(files):
    """Conservative Rust/contract/controller closure; frontend executes current."""
    roots = ('crates/desktop-core/', 'crates/xtask/src/', 'packages/desktop-contracts/',
             'resources/', 'docs/architecture/', '.cargo/')
    exact = {'Cargo.toml', 'Cargo.lock', 'rust-toolchain.toml', 'pnpm-lock.yaml',
             'pnpm-workspace.yaml', 'package.json', 'crates/xtask/Cargo.toml'}
    return sorted(name for name in files if name in exact or name.startswith(roots))


def verify_tool_transition(root, reference, old, new):
    audit = read(bound(root, reference))
    old_tools = {row['name']: row for row in old['toolchain']}
    new_tools = {row['name']: row for row in new['toolchain']}
    audited = {row['name']: row for row in audit['current_tools']}
    comparisons = {row['name']: row for row in audit['comparison']}
    names = {'rustc', 'cargo', 'rustc-toolchain', 'cargo-toolchain', 'node', 'pnpm'}
    require(set(old_tools) == set(new_tools) == set(audited) == set(comparisons) == names,
            'tool transition membership differs')
    for name in names:
        before, after, joined = old_tools[name], new_tools[name], comparisons[name]
        require(before['sha256'] == joined['old_sha256']
                and after['sha256'] == joined['new_sha256'] == audited[name]['sha256'],
                'tool transition audit does not bind actual executables')
        if name in ('rustc', 'cargo'):
            require(before['version'] == after['version'], 'selected Rust version changed')
        else:
            # Historical rustup-resolved identities recorded no version string.
            require(before['sha256'] == after['sha256'], 'actual compiler or JS tool changed')


def validate_impact_selection(root, path, runner_inputs, source=None, files=None):
    """Dated Kirk-to-VM/frontend fix accounting; never creates a passing raw run."""
    record = read(path)
    require(record['schema'] == 'airs-desktop/p11-impact-selection/1'
            and record['runner_inputs'] == runner_inputs and record['reason'],
            'impact selection binding missing')
    before = read(bound(root, record['baseline_source']))
    after = read(bound(root, record['current_source']))
    require(after['files'] == (source_inventory(root) if files is None else files),
            'impact selection current source differs')
    require(inventory_source(after) == runner_inputs['source'], 'impact source fingerprint differs')
    changed = sorted(name for name in set(before['files']) | set(after['files'])
                     if before['files'].get(name) != after['files'].get(name))
    require(changed and changed == record['changed_paths'], 'impact delta differs')
    origin = read(bound(root, record['baseline_candidate']))
    require(origin['source_inventory_sha256'] == digest(before['files'])
            and origin['runner_inputs']['source'] == inventory_source(before),
            'baseline source-check context differs')
    baseline_root = root / D2_SOURCE
    require({str(p.relative_to(baseline_root)): file_identity(p)
             for p in baseline_root.rglob('*') if p.is_file() or p.is_symlink()} == before['files'],
            'D2 source archive membership/content differs')
    baseline_check = validate_check_selection(root, bound(root, record['baseline_check']),
        origin['runner_inputs'], source=baseline_root, files=before['files'])
    baseline_test = validate_run(root, bound(root, record['baseline_test']), 'test',
                                 origin['runner_inputs'], source=baseline_root)
    dependencies = carry_dependencies(before['files'])
    require(dependencies and dependencies == carry_dependencies(after['files'])
            and dependencies == record['dependencies']
            and all(before['files'][name] == after['files'][name] for name in dependencies),
            'carried dependency membership/content changed')
    plans = {kind: command_plan(source or root, kind) for kind in ('check', 'test')}
    labels = {'check': list(IMPACT_CHECKS), 'test': [
        row['label'] for row in plans['test'] if row['argv'][:2] != ['cargo', 'test']
        and not row['label'].startswith('setup/')]}
    require(record['affected'] == labels, 'affected frontend/check selection differs')
    current = {kind: validate_run(root, bound(root, record['current_runs'][kind]), kind,
                                  runner_inputs, labels=labels[kind], source=source)
               for kind in ('check', 'test')}
    require(current['check']['toolchain'] == current['test']['toolchain'],
            'affected runs used different tools')
    verify_tool_transition(root, record['tool_transition'], baseline_test, current['test'])
    baseline_choices = {'check': baseline_check['selected'], 'test': [
        {'needed': needed, 'actual': actual, 'raw': baseline_test,
         'path': bound(root, record['baseline_test'])}
        for needed, actual in zip(command_plan(baseline_root, 'test'), baseline_test['commands'])]}
    choices = {}
    for kind in ('check', 'test'):
        old = {row['needed']['label']: row for row in baseline_choices[kind]}
        fresh = {row['label']: row for row in current[kind]['commands']}
        choices[kind] = []
        for needed in plans[kind]:
            if needed['label'].startswith('setup/'):
                continue
            label = needed['label']
            if label in fresh:
                choice = dict(needed=needed, actual=fresh[label], raw=current[kind],
                              path=bound(root, record['current_runs'][kind]),
                              disposition='executed', reason=None)
            else:
                require(label in old and old[label]['needed']['argv'] == needed['argv'],
                        'carried command was not in the original approved plan')
                choice = dict(old[label], needed=needed, disposition='carried_unaffected',
                              reason=record['reason'])
            choices[kind].append(choice)
    return {'record': record, 'choices': choices, 'origin': origin,
            'dependencies': dependencies}



S2_SOURCE = Path('docs/planning/evidence/P11/source-history/S2')


def validate_helper_source_carry(root, path, runner_inputs, source=None, files=None):
    """Reuse source QA only across a fully enumerated helper/proof-only change."""
    record = read(path)
    require(record['schema'] == 'airs-desktop/p11-helper-source-carry/1'
            and record['runner_inputs'] == runner_inputs and record['reason'], 'helper carry binding missing')
    before = read(bound(root, record['baseline_source']))
    after = read(bound(root, record['current_source']))
    require(after['files'] == (source_inventory(root) if files is None else files)
            and inventory_source(after) == runner_inputs['source'], 'helper carry current source differs')
    archive = root / S2_SOURCE
    require({str(p.relative_to(archive)): file_identity(p) for p in archive.rglob('*')
             if p.is_file() or p.is_symlink()} == before['files'], 'S2 archive membership/content differs')
    changed = sorted(name for name in set(before['files']) | set(after['files'])
                     if before['files'].get(name) != after['files'].get(name))
    allowed = lambda name: (name.startswith(('tools/native-acceptance/', 'tools/p11/'))
        or name in ('.gitattributes', 'AGENTS.md', 'crates/xtask/examples/p11_affected.rs',
                    'docs/planning/refinements/2026-11-04/execution.md'))
    require(changed and changed == record['changed_paths'] and all(allowed(n) for n in changed),
            'undeclared or application-affecting helper carry delta')
    app_names = lambda mapping: {n for n in mapping if any(n == prefix or n.startswith(prefix + '/')
                                                           for prefix in APPLICATION_INPUTS)}
    names = app_names(before['files'])
    require(names and names == app_names(after['files'])
            and all(before['files'][name] == after['files'][name] for name in names),
            'application input membership/content changed across helper carry')
    origin = read(bound(root, record['baseline_candidate']))
    require(origin['source_inventory_sha256'] == digest(before['files'])
            and origin['runner_inputs']['source'] == inventory_source(before)
            and all(origin['runner_inputs'][key] == runner_inputs[key]
                    for key in ('dependencies', 'build_flags')), 'helper carry source/dependencies/flags differ')
    require(record['conditions'] == origin['conditions'], 'helper carry VM conditions differ')
    if files is None:
        require(record['conditions'] == {'platform': platform.platform(), 'hostname': platform.node()},
                'helper carry current VM differs')
    selected = validate_impact_selection(root, bound(root, record['impact_selection']),
        origin['runner_inputs'], source=archive, files=before['files'])
    require(selected['record']['current_source'] == record['baseline_source'],
            'helper carry does not join original QA source')
    covered = set()
    require(record['qa'], 'helper-only changes need affected pure QA')
    for ref in record['qa']:
        qa = read(bound(root, ref))
        require(qa['schema'] == 'airs-desktop/p11-helper-qa/1' and qa['argv']
                and qa['cwd'] == origin['execution_root'] and qa['exit_code'] == 0
                and qa['tests']['passed'] > 0 and qa['tests']['failed'] == 0,
                'helper pure QA failed or provenance missing')
        bound(root, qa['log'])
        require(qa['helper_files'] and all(name in after['files']
                and after['files'][name].get('sha256') == value
                for name, value in qa['helper_files'].items()), 'helper QA input drift')
        if 'source_before' in qa or 'source_after' in qa:
            require(qa.get('source_before') == qa.get('source_after'), 'helper QA inputs changed during execution')
        covered.update(qa['helper_files'])
    require({n for n in changed if n.endswith('.py')} <= covered, 'changed helper lacks affected pure QA')
    for kind in ('check', 'test'):
        for choice in selected['choices'][kind]:
            if choice['disposition'] == 'executed':
                choice['disposition'] = 'reused_exact'
                choice['reason'] = record['reason']
    return {'record': record, 'selection': selected, 'origin': origin}


def verify_impact_coverage(root, qualified, historical=None):
    candidate = read(bound(root, qualified['candidate']))
    source = historical.source(qualified['source_inventory_sha256']) if historical else root
    seal = read(bound(root, qualified['impact_source']))
    require(digest(seal['files']) == qualified['source_inventory_sha256'],
            'impact selection does not bind qualified source')
    reuse = (validate_helper_source_carry(root, bound(root, qualified['helper_source_carry']),
        candidate['runner_inputs'], source=source, files=seal['files'] if historical else None)
        if 'helper_source_carry' in qualified else None)
    selected = (reuse['selection'] if reuse else validate_impact_selection(root,
        bound(root, qualified['impact_selection']), candidate['runner_inputs'],
        source=source, files=seal['files'] if historical else None))
    require(qualified['impact_source'] == (reuse['record']['current_source'] if reuse else
            selected['record']['current_source']), 'impact source reference differs')
    if reuse:
        require(qualified['impact_selection'] == reuse['record']['impact_selection'],
                'helper carry original impact selection differs')
        require(all(candidate[key] == reuse['origin'][key] for key in coverage.EXACT_FIELDS),
                'helper carry application/flags/conditions changed')
    check_choices = {'check:' + row['needed']['label']: row for row in selected['choices']['check']
                     if row['disposition'] != 'reused_exact'}
    modes = {kind + ':' + row['needed']['label']: row['disposition']
             for kind in ('check', 'test') for row in selected['choices'][kind]}
    per_native = (native_origins(root, bound(root, qualified['native_acceptance']),
        bound(root, qualified['native_selection']), candidate) if 'native_selection' in qualified else {})
    require(not per_native or reuse is not None, 'native continuation lacks helper-source transition')
    modes.update({cid: origin['disposition'] for cid, origin in per_native.items()})
    plan = read(bound(root, qualified['plan']))
    verify_plan(source, plan, modes)
    for cid, origin in per_native.items():
        needed = next(row for row in plan['requirements'] if row['id'] == cid)
        require(needed['argv'] == origin['raw']['argv']
                and needed['helper_paths'] == origin['helper_paths'], 'native per-row requirement drift')
    require(all(row.get('dependencies') == selected['dependencies']
                for row in plan['requirements'] if modes.get(row['id']) == 'carried_unaffected'),
            'impact dependency plan differs from conservative closure')
    rows = qualified['coverage']
    require(len(rows) == len(plan['requirements']) and {r['id'] for r in rows}
            == {r['id'] for r in plan['requirements']}, 'impact coverage membership differs')
    test_choices = {'test:' + row['needed']['label']: row for row in selected['choices']['test']}
    if reuse:
        test_choices.update({'check:' + row['needed']['label']: row for row in selected['choices']['check']
                             if row['disposition'] == 'reused_exact'})
    for row in rows:
        if row['id'] in per_native:
            origin = per_native[row['id']]
            raw = origin['raw']
            result = read(bound(root, row['result']))
            native_candidate = (reuse['record']['baseline_candidate']
                if row['id'] == 'C46-N01' and origin['disposition'] == 'reused_exact' and reuse
                else result['candidate'] if origin['disposition'] == 'reused_exact' else qualified['candidate'])
            if origin['disposition'] == 'reused_exact' and row['id'] != 'C46-N01':
                require(read(bound(root, native_candidate)) == equivalent_native_origin(
                    root, origin['raw_path'], raw, candidate, source),
                    'native original helper equivalence context differs')
            require(row['disposition'] == origin['disposition']
                    and result['status'] == 'pass' and result['exit_code'] == 0
                    and result['process_exit_code'] == raw['exit_code']
                    and result['execution_id'] == raw['execution_id'] and result['argv'] == raw['argv']
                    and result['started_utc'] == raw['started_utc'] and result['finished_utc'] == raw['finished_utc']
                    and result['log'] == reference(root, origin['raw_path'])
                    and result['candidate'] == native_candidate
                    and result['raw'] == qualified['native_acceptance'],
                    'native per-row original execution drift')
            continue
        if row['id'] in test_choices:
            choice = test_choices[row['id']]
            raw, actual, path = choice['raw'], choice['actual'], choice['path']
            carried = choice['disposition'] == 'carried_unaffected'
            require(row['disposition'] == choice['disposition']
                    and read(bound(root, row['result'])) == {
                'status': 'pass', 'exit_code': 0, 'execution_id': raw['run_id'],
                'argv': actual['argv'],
                'cwd': selected['origin']['execution_root'] if carried else candidate['execution_root'],
                'started_utc': raw['started_utc'], 'finished_utc': raw['finished_utc'],
                'required_ids': [row['id']], 'log': reference(root, path.parent / actual['log']),
                'candidate': (selected['record']['baseline_candidate'] if carried else
                    reuse['record']['baseline_candidate'] if reuse else qualified['candidate']),
                'raw': reference(root, path), 'evidence': {}},
                'impact Test result changed original provenance')
            continue
        if row['id'] not in check_choices:
            continue
        choice = check_choices[row['id']]
        raw, actual, path = choice['raw'], choice['actual'], choice['path']
        result = read(bound(root, row['result']))
        require(row['disposition'] == choice['disposition'] and result == {
            'status': 'pass', 'exit_code': 0, 'execution_id': raw['run_id'],
            'argv': actual['argv'], 'cwd': selected['origin']['execution_root'],
            'started_utc': raw['started_utc'], 'finished_utc': raw['finished_utc'],
            'required_ids': [row['id']], 'log': reference(root, path.parent / actual['log']),
            'candidate': qualified['candidate'], 'raw': reference(root, path),
            'original_runner_inputs': raw['inputs_before'], 'reason': choice['reason'], 'evidence': {}},
            'impact Check result changed original provenance')
    runtime_plan = read(bound(root, qualified['runtime_plan']))
    require(runtime_plan == dict(plan, requirements=[r for r in plan['requirements']
                                                     if r['id'] not in check_choices]),
            'impact runtime plan membership differs')
    runtime = dict(qualified, plan=qualified['runtime_plan'],
                   coverage=[row for row in rows if row['id'] not in check_choices])
    result = coverage.verify(root, runtime, historical=historical)
    result['required'] += len(check_choices)
    for row in check_choices.values():
        mode = row['disposition']
        result['dispositions'][mode] = result['dispositions'].get(mode, 0) + 1
    return result


def verify_plan(source, plan, check_dispositions=None):
    expected = {}
    for kind in ('check', 'test'):
        for row in command_plan(source, kind):
            # Setup is real, recorded, and validated as part of each raw run.
            if row['label'].startswith('setup/'):
                continue
            expected[f'{kind}:{row["label"]}'] = {
                'kind': 'command', 'argv': row['argv'],
                'label': row['label'],
                'expects_tests': row['expects_tests']}
    native = read(source / 'docs/architecture/native-checks.json')['chunks']
    for chunk in CHUNKS:
        for row in native[chunk]:
            expected[row['id']] = {'kind': 'native'}
    rows = plan['requirements']
    require(len(rows) == len(expected) and {row['id'] for row in rows} == set(expected),
            'P11 proof omits, duplicates or adds required coverage')
    for row in rows:
        require(all(row.get(key) == value for key, value in expected[row['id']].items()),
                'P11 approved requirement changed')
        affected = not (check_dispositions and check_dispositions.get(row['id']) in
                        ('carried_unaffected', 'reused_exact'))
        require(row.get('affected') is affected and row.get('must_execute') is affected,
                'changed P11 coverage cannot be silently carried')


def verify_coverage(root, qualified, historical=None):
    """P11 check selection plus the unchanged generic test/native consumer."""
    if 'impact_selection' in qualified:
        return verify_impact_coverage(root, qualified, historical=historical)
    if 'check_selection' not in qualified:
        source = historical.source(qualified['source_inventory_sha256']) if historical else root
        verify_plan(source, read(bound(root, qualified['plan'])))
        require(all(row['disposition'] == 'executed' for row in qualified['coverage']),
                'changed P11 coverage was not executed')
        return coverage.verify(root, qualified, historical=historical)
    candidate = read(bound(root, qualified['candidate']))
    source = historical.source(qualified['source_inventory_sha256']) if historical else root
    check_source = read(bound(root, qualified['check_source']))
    require(digest(check_source['files']) == qualified['source_inventory_sha256'],
            'check selection source does not bind qualified source')
    selection = validate_check_selection(root, bound(root, qualified['check_selection']),
        candidate['runner_inputs'], source=source,
        files=check_source['files'] if historical else None)
    require(qualified['check_source'] == selection['selection']['current_source'],
            'check selection source reference differs')
    require(selection['selection']['execution_root'] == candidate['execution_root'],
            'check selection original execution root differs from candidate')
    selected = {'check:' + row['needed']['label']: row for row in selection['selected']
                if not row['needed']['label'].startswith('setup/')}
    plan = read(bound(root, qualified['plan']))
    verify_plan(source, plan, {cid: row['disposition'] for cid, row in selected.items()})
    rows = qualified['coverage']
    require(len(rows) == len(plan['requirements']) and {r['id'] for r in rows}
            == {r['id'] for r in plan['requirements']}, 'check coverage membership differs')
    for row in rows:
        if row['id'] not in selected:
            continue
        chosen = selected[row['id']]
        raw, actual, path = chosen['raw'], chosen['actual'], chosen['path']
        result = read(bound(root, row['result']))
        require(row['disposition'] == chosen['disposition'] and result == {
            'status': 'pass', 'exit_code': 0, 'execution_id': raw['run_id'],
            'argv': actual['argv'], 'cwd': selection['selection']['execution_root'],
            'started_utc': raw['started_utc'], 'finished_utc': raw['finished_utc'],
            'required_ids': [row['id']], 'log': reference(root, path.parent / actual['log']),
            'candidate': qualified['candidate'], 'raw': reference(root, path),
            'original_runner_inputs': raw['inputs_before'],
            'reason': chosen['reason'], 'evidence': {}}, 'check result changed original execution binding')
    runtime_plan = read(bound(root, qualified['runtime_plan']))
    require(runtime_plan == dict(plan, requirements=[r for r in plan['requirements'] if r['id'] not in selected]),
            'runtime plan does not match test/native subset')
    runtime = dict(qualified, plan=qualified['runtime_plan'],
                   coverage=[row for row in rows if row['id'] not in selected])
    require(all(row['disposition'] == 'executed' for row in runtime['coverage']),
            'changed P11 test/native coverage was not executed')
    result = coverage.verify(root, runtime, historical=historical)
    result['required'] += len(selected)
    for row in selected.values():
        mode = row['disposition']
        result['dispositions'][mode] = result['dispositions'].get(mode, 0) + 1
    return result


def verify(root, path):
    record = read(path)
    require(record['schema'] == 'airs-desktop/p11-shared-qualification/1', 'P11 proof schema')
    require(set(record['runner_inputs']) == {'source', 'dependencies', 'build_flags'},
            'P11 runner input keys differ')
    require(record['runner_inputs']['source'] == xtask_source(root),
            'current source does not match actual runner source fingerprint')
    seal = read(bound(root, record['source']))
    require(source_inventory(root) == seal['files'], 'P11 source changed after qualification')
    qualified = read(bound(root, record['coverage']))
    require(digest(seal['files']) == qualified['source_inventory_sha256'], 'P11 source binding')
    if 'helper_source_carry' in record:
        validate_helper_source_carry(root, bound(root, record['helper_source_carry']), record['runner_inputs'])
    elif 'impact_selection' in record:
        validate_impact_selection(root, bound(root, record['impact_selection']), record['runner_inputs'])
    else:
        for kind in ('check', 'test'):
            validate_run(root, bound(root, record['runs'][kind]), kind, record['runner_inputs'])
    inputs = read(bound(root, record['fingerprint']))
    require(record['runner_inputs'] == {key: inputs[key] for key in record['runner_inputs']},
            'P11 observed current fingerprint mismatch')
    current = current_fingerprints(root)
    require(record['runner_inputs'] == {key: current[key] for key in record['runner_inputs']},
            'current dependency/toolchain/build flags differ from executed checks')
    result = verify_coverage(root, qualified)
    candidate = read(bound(root, qualified['candidate']))
    from candidate import verify_candidate
    verify_candidate(root, candidate)
    performed = read(bound(root, record['native_execution']))
    require(candidate['helper_closure'] == helper_closure(root, native_driver(root, performed)),
            'P11 native helper membership changed')
    validate_execution(root, performed, candidate,
        acceptance=bound(root, record['native_acceptance']) if 'native_acceptance' in record else None,
        execution=bound(root, record['native_execution']))
    build_path = bound(root, record['build'])
    build = read(build_path)
    require(build['application_inputs_unchanged'] is True
            and all(step['exit_code'] == 0 for step in build['steps']), 'fixture build failed/drifted')
    require([step['argv'] for step in build['steps']] == [
        ['pnpm', '--filter', '@prisma-airs/desktop-contracts', 'typecheck'],
        ['pnpm', '--filter', '@prisma-airs/desktop', 'typecheck'],
        ['cargo', 'run', '--locked', '-p', 'xtask', '--', 'native-prepare', '--phase', 'P11',
         '--target', 'aarch64-apple-darwin', '--features', 'test-fixtures']],
        'missing or changed fixture build invocation')
    require(set(build['application_inputs_before']) == application_files(root),
            'built application input membership differs or is incomplete')
    require(read(build_path.parent / 'native-prepare.json') == read(bound(root, candidate['prepare'])),
            'prepared candidate differs from retained actual build output')
    for step in build['steps']:
        log = (build_path.parent / step['log']).resolve()
        require(log.is_relative_to(root.resolve()) and sha(log) == step['log_sha256'],
                'fixture build log drift')
    changed = [name for name, value in build['application_inputs_before'].items()
               if sha(root / name) != value]
    if changed:
        carry_schema = read(bound(root, record['build_input_carry']))['schema']
        if carry_schema == 'airs-desktop/rust-test-build-carry/1':
            rust_test_carry(root, record, build_path, build, candidate, changed)
        else:
            frontend_test_carry(root, record, build_path, build, candidate, changed)
    else:
        require('build_input_carry' not in record, 'unnecessary or stale build carry')
    baseline = read(bound(root, record['historical_baseline']))
    require(record['historical_baseline']['path'] == 'docs/planning/evidence/P10/completion-proof.json'
            and baseline['schema'] == 'airs-desktop/p10-completion-proof/1', 'missing accepted baseline')
    from review_gate import release_prerequisites
    require(record['historical_standing'] == release_prerequisites(root),
            'actual historical standing drift')
    return {'record': str(path.relative_to(root)), 'sha256': sha(path),
            'verified': result, 'candidate_sha256': qualified['candidate']['sha256'],
            'controller_complete': False}


def make(root, check, tests, fingerprint, prepare, native, execution, build, out,
         carry=None, impact=None, helper_carry=None):
    """Bind existing executions; never create a passing raw run or verdict."""
    require(not out.exists(), 'preserve previous P11 qualification')
    require((root / 'tools/native-acceptance/acceptance_p11.py').is_file(),
            'P11 native driver missing')
    inputs = read(fingerprint)
    runner_inputs = {key: inputs[key] for key in ('source', 'dependencies', 'build_flags')}
    reuse = validate_helper_source_carry(root, helper_carry, runner_inputs) if helper_carry else None
    selection = reuse['selection'] if reuse else validate_impact_selection(root, impact, runner_inputs) if impact else None
    if reuse:
        require(reuse['record']['impact_selection'] == reference(root, impact), 'helper QA selection differs')
    runs = ({} if selection else {kind: validate_run(root, path, kind, runner_inputs)
            for kind, path in [('check', check), ('test', tests)]})
    if selection:
        require(selection['record']['baseline_check'] == reference(root, check)
                and selection['record']['baseline_test'] == reference(root, tests),
                'impact original Check/Test references differ')
    prepared, acceptance, performed = read(prepare), read(native), read(execution)
    require(prepared['subject'] == acceptance['subject'] == 'P11', 'P11 candidate subject')
    require(acceptance['schema'] == 'airs-v2/native-acceptance/1'
            and acceptance['method'] == 'cua' and acceptance['app']['fixture'] is True,
            'not an actual fixture native acceptance')
    require(acceptance['app']['executable_sha256'] == prepared['executable_sha256'],
            'native executable differs from prepared candidate')
    continuation = performed.get('schema') == 'airs-desktop/p11-native-selection/1'
    if not continuation:
        require(performed.get('argv') and performed.get('started_utc') and performed.get('finished_utc'),
                'native execution provenance missing')
    helpers = helper_closure(root, native_driver(root, performed))
    if not continuation:
        require(performed['helper_closure'] == helpers, 'native execution helper closure differs')
    require(prepared['bundle_sha256'] == bundle_hash(root / prepared['bundle']),
            'complete native bundle drift')
    out.mkdir(parents=True)

    def write(name, value):
        path = out / name
        require(not path.exists(), 'preserve previous proof file')
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value, indent=2) + '\n')
        return reference(root, path)

    source = source_inventory(root)
    seal = write('source-seal.json', {'files': source})
    candidate = {'source_inventory_sha256': digest(source), 'execution_root': str(root),
                 'runner_inputs': runner_inputs, 'dependency_fingerprint': inputs['dependencies'],
                 'prepare': reference(root, prepare),
                 'executable_sha256': prepared['executable_sha256'],
                 'bundle_sha256': prepared['bundle_sha256'],
                 'build_flags': {'target': prepared['target'], 'features': prepared['features']},
                 'conditions': {'platform': platform.platform(), 'hostname': platform.node()},
                 'helper_closure': helpers}
    candidate_ref = write('candidate.json', candidate)
    per_native = validate_execution(root, performed, candidate,
        acceptance=native if continuation else None, execution=execution) or {}
    if continuation:
        require(reuse is not None, 'native continuation needs explicit helper-source carry')
    requirements, rows = [], []
    for kind, path in [('check', check), ('test', tests)]:
        raw = runs.get(kind, {})
        selected_check = kind == 'check' and ('selection' in raw or selection is not None)
        choices = (selection['choices'][kind] if selection else raw['selected'] if selected_check else
                   [{'needed': needed, 'actual': actual, 'raw': raw, 'path': path,
                     'disposition': 'executed', 'reason': None}
                    for needed, actual in zip(command_plan(root, kind), raw['commands'])])
        for choice in choices:
            needed, actual = choice['needed'], choice['actual']
            if needed['label'].startswith('setup/'):
                continue
            cid = kind + ':' + needed['label']
            affected = choice['disposition'] == 'executed'
            requirement = dict(id=cid, kind='command', **needed, affected=affected,
                               must_execute=affected, helper_paths=[])
            if selection and choice['disposition'] == 'carried_unaffected':
                requirement['dependencies'] = selection['dependencies']
            requirements.append(requirement)
            origin, origin_path = choice['raw'], choice['path']
            log = reference(root, origin_path.parent / actual['log'])
            result = dict(status='pass', exit_code=0, execution_id=origin['run_id'],
                          argv=actual['argv'], cwd=str(root), started_utc=origin['started_utc'],
                          finished_utc=origin['finished_utc'], required_ids=[cid], log=log,
                          candidate=candidate_ref, raw=reference(root, origin_path), evidence={})
            if selected_check and choice['disposition'] != 'reused_exact':
                result.update(original_runner_inputs=origin['inputs_before'], reason=choice['reason'])
            if reuse and choice['disposition'] == 'reused_exact':
                result['candidate'] = reuse['record']['baseline_candidate']
                result['cwd'] = reuse['origin']['execution_root']
            if selection and kind == 'test' and choice['disposition'] == 'carried_unaffected':
                result['candidate'] = selection['record']['baseline_candidate']
                result['cwd'] = selection['origin']['execution_root']
            result_ref = write('results/' + kind + '-' + needed['label'].replace('/', '-') + '.json', result)
            row = dict(id=cid, disposition=choice['disposition'], result=result_ref)
            if selection and kind == 'test' and choice['disposition'] == 'carried_unaffected':
                deps = selection['dependencies']
                bindings = {name: {'original_sha256': sha(root / D2_SOURCE / name),
                                    'current_sha256': sha(root / name),
                                    'original_file': reference(root, root / D2_SOURCE / name)}
                            for name in deps}
                impact_ref = write('impact/' + needed['label'].replace('/', '-') + '.json', {
                    'file_bindings': bindings, 'unchanged_dependencies': deps,
                    'current_source_inventory_sha256': digest(source),
                    'original_candidate_sha256': selection['record']['baseline_candidate']['sha256']})
                row['impact'] = {'reason': selection['record']['reason'], 'dependencies': deps,
                                 'original_candidate_sha256': selection['record']['baseline_candidate']['sha256'],
                                 'proof': impact_ref}
            rows.append(row)
    expected = {row['id'] for chunk in CHUNKS for row in
                read(root / 'docs/architecture/native-checks.json')['chunks'][chunk]}
    require(len(acceptance['checks']) == len(expected)
            and {row['id'] for row in acceptance['checks']} == expected,
            'native acceptance does not cover exactly the twelve P11 assertions')
    native_contexts = {}
    for row in acceptance['checks']:
        cid = row['id']
        require(row['result'] == 'pass' and row['observation'] and row['evidence'],
                'native assertion not established: ' + cid)
        evidence = {name: sha(root / name) for name in row['evidence']}
        origin = per_native.get(cid)
        actual = origin['raw'] if origin else performed
        raw_path = origin['raw_path'] if origin else execution
        mode = origin['disposition'] if origin else 'executed'
        require(str(execution.relative_to(root)) in evidence and str(raw_path.relative_to(root)) in evidence,
                'native row omits original execution provenance')
        requirements.append(dict(id=cid, kind='native', argv=actual['argv'], affected=mode == 'executed',
            must_execute=mode == 'executed', helper_paths=origin['helper_paths'] if origin else sorted(helpers)))
        native_candidate = candidate_ref
        if mode == 'reused_exact':
            if cid == 'C46-N01':
                native_candidate = reuse['record']['baseline_candidate']
            else:
                key = sha(raw_path)
                if key not in native_contexts:
                    native_contexts[key] = write('native-origins/' + actual['execution_id'] + '-' + key[:8] + '.json',
                        equivalent_native_origin(root, raw_path, actual, candidate, root))
                native_candidate = native_contexts[key]
        result = dict(status='pass', exit_code=0, execution_id=actual['execution_id'],
                      argv=actual['argv'], cwd=str(root), started_utc=actual['started_utc'],
                      finished_utc=actual['finished_utc'], required_ids=[cid],
                      log=reference(root, raw_path), candidate=native_candidate,
                      raw=reference(root, native), evidence=evidence)
        if continuation:
            result['process_exit_code'] = actual['exit_code']
        rows.append(dict(id=cid, disposition=mode, result=write('results/' + cid + '.json', result)))
    plan = write('required-coverage.json', {'schema': 'airs-desktop/required-coverage/1',
                 'subject': 'P11', 'requirements': requirements})
    qualified_record = {'schema': 'airs-desktop/qualified-coverage/1',
                      'plan': plan, 'candidate': candidate_ref,
                      'source_inventory_sha256': digest(source), 'coverage': rows}
    if selection:
        custom_checks = {row['id'] for row in rows if row['id'].startswith('check:')
                         and row['disposition'] != 'reused_exact'}
        qualified_record.update(impact_selection=reference(root, impact),
            impact_source=reuse['record']['current_source'] if reuse else selection['record']['current_source'],
            runtime_plan=write('runtime-required-coverage.json', {
                'schema': 'airs-desktop/required-coverage/1', 'subject': 'P11',
                'requirements': [r for r in requirements if r['id'] not in custom_checks]}))
        if reuse:
            qualified_record['helper_source_carry'] = reference(root, helper_carry)
    elif 'selection' in runs['check']:
        qualified_record.update(check_selection=reference(root, check),
            check_source=runs['check']['selection']['current_source'],
            runtime_plan=write('runtime-required-coverage.json', {
                'schema': 'airs-desktop/required-coverage/1', 'subject': 'P11',
                'requirements': [r for r in requirements if not r['id'].startswith('check:')]}))
    if continuation:
        qualified_record.update(native_acceptance=reference(root, native), native_selection=reference(root, execution))
    qualified = write('coverage.json', qualified_record)
    record = {'schema': 'airs-desktop/p11-shared-qualification/1', 'source': seal,
              'coverage': qualified, 'runs': {'check': reference(root, check), 'test': reference(root, tests)},
              'runner_inputs': runner_inputs, 'fingerprint': reference(root, fingerprint),
              'native_execution': reference(root, execution),
              'build': reference(root, build),
              'historical_baseline': reference(root, root / 'docs/planning/evidence/P10/completion-proof.json'),
              'historical_use': 'Original P10 standing/artifact bindings retained. No old result is relabelled current and no RC1 byte equality is claimed. P11 executes the changed-flow requirements on one shared candidate.',
              'controller_complete': False}
    if selection:
        record['impact_selection'] = reference(root, impact)
    if continuation:
        record['native_acceptance'] = reference(root, native)
    if reuse:
        record['helper_source_carry'] = reference(root, helper_carry)
    if carry is not None:
        record['build_input_carry'] = reference(root, carry)
    from review_gate import release_prerequisites
    record['historical_standing'] = release_prerequisites(root)
    record_ref = write('qualification.json', record)
    return verify(root, root / record_ref['path'])


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    actions = parser.add_subparsers(dest='action', required=True)
    inspect = actions.add_parser('verify')
    inspect.add_argument('record', type=Path)
    create = actions.add_parser('make')
    for flag in ('check', 'tests', 'fingerprint', 'prepare', 'native', 'execution', 'build', 'out'):
        create.add_argument('--' + flag, type=Path, required=True)
    create.add_argument('--build-input-carry', type=Path)
    create.add_argument('--impact-selection', type=Path)
    create.add_argument('--helper-source-carry', type=Path)
    args = parser.parse_args()
    root = Path.cwd().resolve()
    if args.action == 'verify':
        result = verify(root, args.record.resolve())
    else:
        result = make(root, *[getattr(args, key).resolve() for key in
                             ('check', 'tests', 'fingerprint', 'prepare', 'native', 'execution', 'build', 'out')],
                      carry=args.build_input_carry.resolve() if args.build_input_carry else None,
                      impact=args.impact_selection.resolve() if args.impact_selection else None,
                      helper_carry=args.helper_source_carry.resolve() if args.helper_source_carry else None)
    print(json.dumps(result, indent=2))
