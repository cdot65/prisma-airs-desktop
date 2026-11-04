#!/usr/bin/env python3
"""Finish first-uncompleted credentials and remaining shell/warning assertions."""
import argparse
import json
import os
from pathlib import Path
import platform
import sys
import tempfile
import time

import acceptance_p11 as core
import acceptance_p11_resume as prior_driver
import acceptance_p11_continue as completed_driver
import acceptance_p11_credentials as credentials
import acceptance_p11_roles as roles
import acceptance_p11_api as api
import acceptance_p11_settings as settings
import acceptance_p11_shell as shell
import acceptance_p11_setup as setup

ROOT = core.ROOT
C46 = 'C46-N01'


def relevant_c46(root=ROOT):
    paths = set(core._binding.helper_closure(root, 'acceptance_p11_setup'))
    paths.add('tools/native-acceptance/acceptance_p11.py')
    if 'tools/native-acceptance/acceptance_p11_api.py' in paths:
        raise RuntimeError('C46 dependency closure unexpectedly includes API helper')
    return sorted(paths)


def inspect_chain(paths, prepare, work, state_path):
    """Validate both failed runs and latest state before any new native action."""
    require = core.Context.require
    require(len(paths) >= 2, 'continuation requires its ordered prior observation chain')
    if work.is_symlink(): raise RuntimeError('resume work cannot be a symlink')
    work = work.resolve()
    require(work.parent == Path(tempfile.gettempdir()).resolve() and work.name.startswith('airs-p11-owned-')
            and work.is_dir() and work.stat().st_uid == os.getuid(), 'unowned continuation work')
    prepared = json.loads(prepare.read_text())
    completed = set()
    current = core._binding.helper_closure(ROOT, 'acceptance_p11_finish')
    raws = []
    prior_refs = []
    for index, path in enumerate(paths):
        path = path.resolve(); raw = json.loads(path.read_text())
        require(raw['schema'] == 'airs-desktop/p11-native-observations/1'
                and raw['subject'] == 'P11' and raw['status'] == 'failed'
                and raw['exit_code'] == 1 and raw['complete'] is False and raw['errors'],
                'prior process outcome must remain failed')
        require(raw['host'] == platform.platform()
                and raw['executable_sha256'] == prepared['executable_sha256']
                and raw['bundle_sha256'] == prepared['bundle_sha256']
                and raw['binding']['prepare']['sha256'] == core.sha(prepare)
                and raw['binding']['native_inventory_sha256'] == core.sha(ROOT/'docs/architecture/native-checks.json'),
                'prior native artifact/platform/prepare differs')
        for name, digest in raw['artifact_sha256'].items():
            artifact = (ROOT/name).resolve()
            require(artifact.is_relative_to(path.parent) and core.sha(artifact) == digest,
                    'prior immutable native artifact changed')
        if index == 0:
            require([row['id'] for row in raw['checks']] == ['C46-N01'], 'initial setup scope differs')
        else:
            admission = raw['continuation']
            require(admission.get('origins', [admission['observations']]) == prior_refs
                    and sorted(admission['completed_ids']) == sorted(completed)
                    and sorted(admission['remaining_ids']) == sorted(set(core.inventory())-completed),
                    'prior observation chain is inconsistent')
        for row in raw['checks']:
            require(row['id'] not in completed and row['facts']
                    and all(value is True for value in row['facts'].values()),
                    'prior completed assertion missing, duplicated or failed')
            paths_in_scope = relevant_c46() if index == 0 else sorted(raw['helper_closure'])
            for name in paths_in_scope:
                require(raw['helper_closure'].get(name) == current.get(name) == core.sha(ROOT/name),
                        'prior relevant helper changed: '+name)
            completed.add(row['id'])
        raws.append(raw)
        prior_refs.append({'path': str(path.relative_to(ROOT)), 'sha256': core.sha(path)})
    require(completed and completed < set(core.inventory()), 'no unfinished native assertion remains')
    latest = paths[-1].resolve()
    require(raws[-1]['continuation']['work'] == str(work), 'prior continuation work differs')
    state_file = latest.parent/'fixture-state-final.json'
    require(raws[-1]['artifact_sha256'].get(str(state_file.relative_to(ROOT))) == core.sha(state_file),
            'latest final fixture state is not bound')
    state = json.loads(state_path.read_text())
    require(state == json.loads(state_file.read_text()), 'fixture state changed since latest failed run')
    return raws[-1], state


class FinishContext(completed_driver.ContinueContext):
    pass



def restored_context(args):
    import fixture_app as fx
    import acceptance_c08 as c8
    prepared, bundle = core.verify_prepare(args.prepare.resolve())
    previous, state = inspect_chain(args.previous, args.prepare, args.work, Path(fx.DATA)/'state.json')
    if prepared['executable_sha256'] != previous['executable_sha256'] or prepared['bundle_sha256'] != previous['bundle_sha256']:
        raise RuntimeError('resume app artifact differs')
    work = args.work.resolve()
    guard_files = ['scan/ca.pem', 'scan/leaf.pem', 'scan/leaf.key', 'scan/mode.json',
                   'mgmt/mode.json', 'mgmt/issued.json', 'mgmt/idp-key.pem',
                   'academy/ca.pem', 'academy/leaf.pem', 'academy/leaf.key',
                   'academy/mode.json', 'academy/catalog.json']
    for name in guard_files:
        file = work/name
        if not file.is_file() or file.is_symlink() or file.stat().st_uid != os.getuid():
            raise RuntimeError('owned fixture input missing: '+name)
    expected_roots = (work/'scan/ca.pem').read_bytes()+b'\n'+(work/'academy/ca.pem').read_bytes()
    if Path(fx.ROOTS).read_bytes() != expected_roots:
        raise RuntimeError('installed owned trust roots differ from retained CAs')
    prior_inputs = previous['continuation']
    if core.sha(fx.ROOTS) != prior_inputs['installed_roots_sha256']:
        raise RuntimeError('owned trust roots changed since previous admission')
    for name in guard_files:
        if name.endswith(('.pem', '.key')) or name == 'academy/catalog.json':
            if core.sha(work/name) != prior_inputs['owned_inputs_sha256'][name]:
                raise RuntimeError('retained owned certificate/catalog changed: '+name)
    if Path(c8.MGMT_ORIGIN_FILE).read_text().strip() != 'https://localhost:62780' or (Path(fx.DATA)/'fixture-academy-origin').read_text().strip() != 'https://localhost:62781':
        raise RuntimeError('retained management/Academy origins changed')
    ctx = FinishContext.__new__(FinishContext)  # no new scratch root or reset
    ctx.prepare_path, ctx.prepared, ctx.bundle = args.prepare.resolve(), prepared, bundle
    ctx.out = args.out.resolve(); ctx.out.mkdir(parents=True, exist_ok=False)
    ctx.work, ctx.log, ctx.rows, ctx.errors, ctx.artifacts = work, [], {}, [], {}
    ctx.fixture_root_owned = False
    ctx.argv = [sys.executable, *sys.argv]
    ctx.argv_source = 'sys.executable + sys.argv'
    ctx.started = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
    ctx.execution_exit_code = 1
    ctx.app = ctx.scan = ctx.mgmt = ctx.academy = None
    ctx.closure = core._binding.helper_closure(ROOT, 'acceptance_p11_finish')
    ctx.binding = dict(previous['binding'], helper_closure=ctx.closure,
                       conditions={'platform': platform.platform(), 'hostname': platform.node()})
    tenant = next(t for t in state['tenants'] if t['label'] == 'P11 Northwind')
    warning = next(w for group in state['tenantWarnings'] if group['tenant'] == tenant['id']
                   for w in group['warnings'] if w['product'] == 'modelSkill')
    ctx.state = {'bootstrap_tenant': 'P11 Northwind', 'spare_tenant': 'P11 Secondary',
                 'bootstrap_tenant_id': tenant['id'], 'original_supply_chain_warning': warning,
                 'scan_absent': True}
    ctx.continuation = {'observations': {'path': str(args.previous[-1].resolve().relative_to(ROOT)),
                                         'sha256': core.sha(args.previous[-1])},
                        'origins': [{'path': str(path.resolve().relative_to(ROOT)), 'sha256': core.sha(path)} for path in args.previous],
                        'completed_ids': sorted({row['id'] for path in args.previous for row in json.loads(path.read_text())['checks']}),
                        'remaining_ids': sorted(set(core.inventory())-{row['id'] for path in args.previous for row in json.loads(path.read_text())['checks']}),
                        'work': str(work), 'fixture_state_semantically_equal': True,
                        'owned_inputs_sha256': {name: core.sha(work/name) for name in guard_files},
                        'installed_roots_sha256': core.sha(fx.ROOTS),
                        'certificate_generation_or_install_executed': False}
    ctx.save('resume-preconditions', ctx.continuation)
    # Restore only existing server state. Constructors/install/mode are forbidden.
    ctx.scan = fx.ScanEndpoint.__new__(fx.ScanEndpoint)
    ctx.scan.dir, ctx.scan.key, ctx.scan.port, ctx.scan.proc = str(work/'scan'), c8.SCAN_KEY, fx.free_port(), None
    ctx.mgmt = c8.MgmtEndpoint.__new__(c8.MgmtEndpoint)
    ctx.mgmt.certs, ctx.mgmt.dir, ctx.mgmt.port, ctx.mgmt.proc = str(work/'scan'), str(work/'mgmt'), 62780, None
    ctx.academy = fx.AcademyEndpoint.__new__(fx.AcademyEndpoint)
    ctx.academy.dir, ctx.academy.port, ctx.academy.proc = str(work/'academy'), 62781, None
    return ctx


def execute(args):
    import fixture_app as fx
    if not os.environ.get('AIRS_NATIVE_ORCHESTRATOR'):
        raise RuntimeError('set AIRS_NATIVE_ORCHESTRATOR to the actual VM UI operator')
    ctx = restored_context(args)
    os.environ['AIRS_NATIVE_PROCESS_RECORD'] = str(ctx.out/'candidate-processes.json')
    offsets = {}
    original_sh = fx.sh
    def checked_sh(argv, **kwargs):
        result = original_sh(argv, **kwargs)
        if Path(str(argv[0])).name == 'osascript' and result.returncode != 0:
            raise RuntimeError('System Events transport rejected owned native input')
        return result
    fx.sh = checked_sh
    try:
        ctx.app = fx.App(str(ctx.bundle), str(ctx.work), ctx.log)
        candidate = ctx.app.candidate
        ctx.require(not candidate.matching(candidate.app_path) and not candidate.matching(candidate.engine_path),
                    'unowned fixture process exists; refusing resume')
        ctx.fixture_root_owned = True
        for name, endpoint in [('scan', ctx.scan), ('management', ctx.mgmt), ('academy', ctx.academy)]:
            offsets[name] = len(endpoint.requests())
            endpoint.start()
        ctx.save('endpoint-history-offsets', offsets)
        ctx.app.launch()
        completed = set(ctx.continuation['completed_ids'])
        ctx.require('C49-N02' in completed and 'C49-N01' in completed
                    and all('C48-N'+n in completed for n in ('01', '02', '03')),
                    'finish starts after the already completed API/cards/theme assertions')
        if 'C49-N03' not in completed:
            # The failed unfinished credential assertion left this fixture-only
            # key. Remove it through normal UI before replaying that assertion.
            settings._settings(ctx)
            before = ctx.fixture_state()
            bootstrap_id = ctx.state['bootstrap_tenant_id']
            ctx.require(before['activeTenant'] == bootstrap_id, 'bootstrap tenant is not active')
            management = [row for row in before['credentials'] if row.get('class') == 'management']
            remove = ctx.app.node(ctx.app.tree(), 'AXButton', 'Remove scan key')
            if remove:
                ctx.press('AXButton', 'Remove scan key')
                ctx.text('No scan key.')
            after = ctx.wait_state(lambda state:
                not state.get('pendingCleanup') and not any(
                    row.get('class', 'scan') == 'scan' and row['tenant'] == bootstrap_id
                    for row in state['credentials']), 'unfinished owned scan key cleanup did not settle')
            ctx.require([row for row in after['credentials'] if row.get('class') == 'management'] == management
                        and after['tenants'] == before['tenants'] and after['activeTenant'] == bootstrap_id,
                        'credential replay precondition changed management or tenant metadata')
            ctx.save('c49-owned-scan-replay-precondition', {
                'observations': ctx.continuation['observations'], 'removed_through_normal_ui': bool(remove),
                'management_record_ids_preserved': [row['record'] for row in management],
                'management_metadata_unchanged': True, 'tenant_metadata_unchanged': True,
                'bootstrap_scan_key_absent_and_cleanup_settled': True})
            credentials.run(ctx, ctx.state['bootstrap_tenant'])
        shell_ids = {'C47-N01', 'C47-N02', 'C47-N03'}
        warning_ids = {'C46-N02', 'C46-N03'}
        ctx.require(not (completed & shell_ids) or shell_ids <= completed,
                    'partly completed shell group needs an explicitly scoped next slice')
        ctx.require(not (completed & warning_ids) or warning_ids <= completed,
                    'partly completed warning group needs an explicitly scoped next slice')
        if not shell_ids <= completed: shell.run(ctx)
        if not warning_ids <= completed: setup.warning_flows(ctx)
        core.verify_prepare(ctx.prepare_path)
        ctx.require(core._binding.helper_closure(ROOT, 'acceptance_p11_finish') == ctx.closure,
                    'resume helper changed during execution')
        ctx.require(set(ctx.rows) == set(ctx.continuation['remaining_ids'])
                    and all(value is True for row in ctx.rows.values() for value in row['facts'].values()),
                    'unfinished native facts remain incomplete')
        ctx.execution_exit_code = 0
    except BaseException as error:
        ctx.execution_exit_code = 130 if isinstance(error, KeyboardInterrupt) else 1
        ctx.errors.append({'type': type(error).__name__, 'message': str(error)})
        if ctx.app:
            try: ctx.save('failure-tree')
            except Exception: pass
    finally:
        if ctx.fixture_root_owned:
            for name, endpoint in [('management', ctx.mgmt), ('scan', ctx.scan), ('academy', ctx.academy)]:
                try:
                    ctx.save(name+'-requests', {'prior_count': offsets.get(name, 0),
                        'new_requests': endpoint.requests()[offsets.get(name, 0):]})
                except Exception as error:
                    ctx.errors.append({'type': type(error).__name__, 'message': str(error)})
            try:
                fx.stop_app(); fx.stop_engines()
                ctx.save('fixture-state-final', ctx.fixture_state())
            except Exception as error:
                ctx.errors.append({'type': type(error).__name__, 'message': str(error)})
        for endpoint in (ctx.academy, ctx.mgmt, ctx.scan):
            try: endpoint.stop()
            except Exception as error:
                ctx.errors.append({'type': type(error).__name__, 'message': 'owned endpoint teardown: '+str(error)})
        if ctx.errors and ctx.execution_exit_code == 0: ctx.execution_exit_code = 1
        raw = ctx.observations(not ctx.errors and ctx.execution_exit_code == 0)
        raw['continuation'] = ctx.continuation
        core.write_new(ctx.out/'observations.json', raw)
    return ctx.execution_exit_code


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--previous', type=Path, nargs='+', required=True)
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--prepare', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    raise SystemExit(execute(parser.parse_args()))
