"""P11's shared candidate proof. Reuses the existing strict raw-evidence consumer.

One actual phase run remains a phase run when four chunk reviews consume it.
This module neither executes tests nor assigns scores or ledger completions.
"""
from pathlib import Path
import json
import hashlib
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


def validate_execution(root, performed, candidate):
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


def validate_run(root, path, kind, runner_inputs):
    raw = read(path)
    require(raw['schema'] == 'airs-v2/run/1' and raw['subject'] == 'P11'
            and raw['kind'] == kind, 'not an actual P11 run of the required kind')
    require(raw['status'] == 'passed' and raw['failure'] is None,
            'P11 run is not passing')
    require(raw['inputs_before'] == raw['inputs_after'] == runner_inputs,
            'P11 run inputs differ or changed')
    expected = command_plan(root, kind)
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


def verify_plan(source, plan):
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
        require(row.get('affected') is True and row.get('must_execute') is True,
                'changed P11 coverage cannot be silently carried')


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
    plan = read(bound(root, qualified['plan']))
    verify_plan(root, plan)
    for kind in ('check', 'test'):
        validate_run(root, bound(root, record['runs'][kind]), kind, record['runner_inputs'])
    inputs = read(bound(root, record['fingerprint']))
    require(record['runner_inputs'] == {key: inputs[key] for key in record['runner_inputs']},
            'P11 observed current fingerprint mismatch')
    current = current_fingerprints(root)
    require(record['runner_inputs'] == {key: current[key] for key in record['runner_inputs']},
            'current dependency/toolchain/build flags differ from executed checks')
    result = coverage.verify(root, qualified)
    candidate = read(bound(root, qualified['candidate']))
    from candidate import verify_candidate
    verify_candidate(root, candidate)
    require(candidate['helper_closure'] == helper_closure(root, 'acceptance_p11'),
            'P11 native helper membership changed')
    performed = read(bound(root, record['native_execution']))
    validate_execution(root, performed, candidate)
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
        frontend_test_carry(root, record, build_path, build, candidate, changed)
    else:
        require('build_input_carry' not in record, 'unnecessary or stale build carry')
    require(all(row['disposition'] == 'executed' for row in qualified['coverage']),
            'P11 changed-flow results must be actual shared executions')
    baseline = read(bound(root, record['historical_baseline']))
    require(record['historical_baseline']['path'] == 'docs/planning/evidence/P10/completion-proof.json'
            and baseline['schema'] == 'airs-desktop/p10-completion-proof/1', 'missing accepted baseline')
    from review_gate import release_prerequisites
    require(record['historical_standing'] == release_prerequisites(root),
            'actual historical standing drift')
    return {'record': str(path.relative_to(root)), 'sha256': sha(path),
            'verified': result, 'candidate_sha256': qualified['candidate']['sha256'],
            'controller_complete': False}


def make(root, check, tests, fingerprint, prepare, native, execution, build, out, carry=None):
    """Bind existing executions; never create a passing raw run or verdict."""
    require(not out.exists(), 'preserve previous P11 qualification')
    require((root / 'tools/native-acceptance/acceptance_p11.py').is_file(),
            'P11 native driver missing')
    inputs = read(fingerprint)
    runner_inputs = {key: inputs[key] for key in ('source', 'dependencies', 'build_flags')}
    runs = {kind: validate_run(root, path, kind, runner_inputs)
            for kind, path in [('check', check), ('test', tests)]}
    prepared, acceptance, performed = read(prepare), read(native), read(execution)
    require(prepared['subject'] == acceptance['subject'] == 'P11', 'P11 candidate subject')
    require(acceptance['schema'] == 'airs-v2/native-acceptance/1'
            and acceptance['method'] == 'cua' and acceptance['app']['fixture'] is True,
            'not an actual fixture native acceptance')
    require(acceptance['app']['executable_sha256'] == prepared['executable_sha256'],
            'native executable differs from prepared candidate')
    require(performed.get('argv') and performed.get('started_utc') and performed.get('finished_utc'),
            'native execution provenance missing')
    helpers = helper_closure(root, 'acceptance_p11')
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
    validate_execution(root, performed, candidate)
    requirements, rows = [], []
    for kind, path in [('check', check), ('test', tests)]:
        raw = runs[kind]
        for needed, actual in zip(command_plan(root, kind), raw['commands']):
            if needed['label'].startswith('setup/'):
                continue
            cid = kind + ':' + needed['label']
            requirements.append(dict(id=cid, kind='command', **needed, affected=True,
                                     must_execute=True, helper_paths=[]))
            log = reference(root, path.parent / actual['log'])
            result = dict(status='pass', exit_code=0, execution_id=raw['run_id'],
                          argv=actual['argv'], cwd=str(root), started_utc=raw['started_utc'],
                          finished_utc=raw['finished_utc'], required_ids=[cid], log=log,
                          candidate=candidate_ref, raw=reference(root, path), evidence={})
            result_ref = write('results/' + kind + '-' + needed['label'].replace('/', '-') + '.json', result)
            rows.append(dict(id=cid, disposition='executed', result=result_ref))
    expected = {row['id'] for chunk in CHUNKS for row in
                read(root / 'docs/architecture/native-checks.json')['chunks'][chunk]}
    require(len(acceptance['checks']) == len(expected)
            and {row['id'] for row in acceptance['checks']} == expected,
            'native acceptance does not cover exactly the twelve P11 assertions')
    for row in acceptance['checks']:
        cid = row['id']
        require(row['result'] == 'pass' and row['observation'] and row['evidence'],
                'native assertion not established: ' + cid)
        evidence = {name: sha(root / name) for name in row['evidence']}
        require(str(execution.relative_to(root)) in evidence,
                'native row omits original execution provenance')
        requirements.append(dict(id=cid, kind='native', argv=performed['argv'], affected=True,
                                 must_execute=True, helper_paths=sorted(helpers)))
        result = dict(status='pass', exit_code=performed['exit_code'], execution_id=performed['execution_id'],
                      argv=performed['argv'], cwd=str(root), started_utc=performed['started_utc'],
                      finished_utc=performed['finished_utc'], required_ids=[cid],
                      log=reference(root, execution), candidate=candidate_ref,
                      raw=reference(root, native), evidence=evidence)
        rows.append(dict(id=cid, disposition='executed', result=write('results/' + cid + '.json', result)))
    plan = write('required-coverage.json', {'schema': 'airs-desktop/required-coverage/1',
                 'subject': 'P11', 'requirements': requirements})
    qualified = write('coverage.json', {'schema': 'airs-desktop/qualified-coverage/1',
                      'plan': plan, 'candidate': candidate_ref,
                      'source_inventory_sha256': digest(source), 'coverage': rows})
    record = {'schema': 'airs-desktop/p11-shared-qualification/1', 'source': seal,
              'coverage': qualified, 'runs': {'check': reference(root, check), 'test': reference(root, tests)},
              'runner_inputs': runner_inputs, 'fingerprint': reference(root, fingerprint),
              'native_execution': reference(root, execution),
              'build': reference(root, build),
              'historical_baseline': reference(root, root / 'docs/planning/evidence/P10/completion-proof.json'),
              'historical_use': 'Original P10 standing/artifact bindings retained. No old result is relabelled current and no RC1 byte equality is claimed. P11 executes the changed-flow requirements on one shared candidate.',
              'controller_complete': False}
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
    args = parser.parse_args()
    root = Path.cwd().resolve()
    if args.action == 'verify':
        result = verify(root, args.record.resolve())
    else:
        result = make(root, *[getattr(args, key).resolve() for key in
                             ('check', 'tests', 'fingerprint', 'prepare', 'native', 'execution', 'build', 'out')],
                      carry=args.build_input_carry.resolve() if args.build_input_carry else None)
    print(json.dumps(result, indent=2))
