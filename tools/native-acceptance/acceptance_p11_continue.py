#!/usr/bin/env python3
"""Continue seven unfinished assertions; preserve five completed observations."""
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


def previous_scope(check_id):
    if check_id == 'C46-N01':
        return relevant_c46()
    helper = 'acceptance_p11_api' if check_id.startswith('C48-') else 'acceptance_p11_settings'
    paths = set(core._binding.helper_closure(ROOT, helper))
    paths.update(core._binding.helper_closure(ROOT, 'fixture_app'))
    paths.update(core._binding.helper_closure(ROOT, 'acceptance_c29'))
    paths.update(['tools/native-acceptance/acceptance_p11.py',
                  'tools/native-acceptance/acceptance_p11_resume.py'])
    return sorted(paths)


def inspect_chain(paths, prepare, work, state_path):
    """Validate both failed runs and latest state before any new native action."""
    require = core.Context.require
    require(len(paths) == 2, 'this continuation requires original06 and original07')
    if work.is_symlink(): raise RuntimeError('resume work cannot be a symlink')
    work = work.resolve()
    require(work.parent == Path(tempfile.gettempdir()).resolve() and work.name.startswith('airs-p11-owned-')
            and work.is_dir() and work.stat().st_uid == os.getuid(), 'unowned continuation work')
    prepared = json.loads(prepare.read_text())
    completed = set()
    current = core._binding.helper_closure(ROOT, 'acceptance_p11_continue')
    raws = []
    for path in paths:
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
        for row in raw['checks']:
            require(row['id'] not in completed and row['facts']
                    and all(value is True for value in row['facts'].values()),
                    'prior completed assertion missing, duplicated or failed')
            for name in previous_scope(row['id']):
                require(raw['helper_closure'].get(name) == current.get(name) == core.sha(ROOT/name),
                        'prior relevant helper changed: '+name)
            completed.add(row['id'])
        raws.append(raw)
    expected = {'C46-N01', 'C48-N01', 'C48-N02', 'C48-N03', 'C49-N01'}
    require(completed == expected, 'continue from the first unfinished C49 theme assertion only')
    latest = paths[-1].resolve()
    require(raws[-1]['continuation']['observations'] ==
            {'path': str(paths[0].resolve().relative_to(ROOT)), 'sha256': core.sha(paths[0])}
            and raws[-1]['continuation']['work'] == str(work), 'prior continuation chain/work differs')
    state_file = latest.parent/'fixture-state-final.json'
    require(raws[-1]['artifact_sha256'].get(str(state_file.relative_to(ROOT))) == core.sha(state_file),
            'latest final fixture state is not bound')
    state = json.loads(state_path.read_text())
    require(state == json.loads(state_file.read_text()), 'fixture state changed since latest failed run')
    return raws[-1], state


class ContinueContext(core.Context):
    def press(self, role, label, timeout=12):
        return roles.press(self, super().press, role, label, timeout)



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
    ctx = ContinueContext.__new__(ContinueContext)  # no new scratch root or reset
    ctx.prepare_path, ctx.prepared, ctx.bundle = args.prepare.resolve(), prepared, bundle
    ctx.out = args.out.resolve(); ctx.out.mkdir(parents=True, exist_ok=False)
    ctx.work, ctx.log, ctx.rows, ctx.errors, ctx.artifacts = work, [], {}, [], {}
    ctx.fixture_root_owned = False
    ctx.argv = [sys.executable, *sys.argv]
    ctx.argv_source = 'sys.executable + sys.argv'
    ctx.started = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
    ctx.execution_exit_code = 1
    ctx.app = ctx.scan = ctx.mgmt = ctx.academy = None
    ctx.closure = core._binding.helper_closure(ROOT, 'acceptance_p11_continue')
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
                        'completed_ids': sorted({'C46-N01', 'C48-N01', 'C48-N02', 'C48-N03', 'C49-N01'}),
                        'remaining_ids': sorted(set(core.inventory())-{'C46-N01', 'C48-N01', 'C48-N02', 'C48-N03', 'C49-N01'}),
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
        # Only the first unfinished assertion is replayed. Its old partial
        # palette captures remain immutable, but lost locals are not invented.
        cards = args.previous[-1].resolve().parent/'c49-settings-cards.json'
        tree = json.loads(cards.read_text())
        def original_value(label):
            values = [node.get('value') for node in tree
                      if node.get('role') == 'AXPopUpButton' and node.get('label') == label]
            ctx.require(len(values) == 1, 'prior Settings preference was not captured uniquely')
            return values[0]
        appearance, theme = original_value('Appearance'), original_value('Theme')
        ctx.require(appearance in ('Match system', 'Light', 'Dark') and theme in ('Prisma AIRS', 'Gruvbox'),
                    'prior captured Settings preferences are invalid')
        windows = [node.get('size') for node in tree if node.get('role') == 'AXWindow']
        ctx.save('settings-initial-preferences-origin', {
            'observations': ctx.continuation['observations'],
            'evidence': {'path': str(cards.relative_to(ROOT)), 'sha256': core.sha(cards)},
            'appearance': appearance, 'theme': theme, 'window': windows[0] if windows else None,
            'meaning': 'Original N01 baseline used for restoration; partial N02 facts are not relabelled.'})
        settings._zoom(ctx)
        settings._themes(ctx, appearance, theme)
        settings._tenant_controls(ctx, ctx.state['bootstrap_tenant'])
        settings._zoom(ctx)
        if windows: settings._resize(ctx, *windows[0])
        ctx.navigate('Home')
        shell.run(ctx)
        setup.warning_flows(ctx)
        core.verify_prepare(ctx.prepare_path)
        ctx.require(core._binding.helper_closure(ROOT, 'acceptance_p11_continue') == ctx.closure,
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
