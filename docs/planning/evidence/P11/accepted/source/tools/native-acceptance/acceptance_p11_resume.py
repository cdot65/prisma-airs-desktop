#!/usr/bin/env python3
"""Resume the first unfinished P11 slice; never repeat completed setup."""
import argparse
import json
import os
from pathlib import Path
import platform
import sys
import tempfile
import time

import acceptance_p11 as core
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


def inspect_previous(path, prepare, work, state_path):
    """Pure admission checks; no app, endpoint, certificate or vault operation."""
    if work.is_symlink(): raise RuntimeError('resume work cannot be a symlink')
    path, work = path.resolve(), work.resolve()
    raw = json.loads(path.read_text())
    require = core.Context.require
    require(raw['schema'] == 'airs-desktop/p11-native-observations/1'
            and raw['subject'] == 'P11' and raw['status'] == 'failed'
            and raw['exit_code'] == 1 and raw['complete'] is False,
            'only the failed first-unfinished API attempt may resume')
    require(len(raw['checks']) == 1 and raw['checks'][0]['id'] == C46
            and raw['checks'][0]['facts']
            and all(value is True for value in raw['checks'][0]['facts'].values()),
            'completed C46 facts missing; setup is not eligible for reuse')
    require(raw['host'] == platform.platform(), 'resume must use the same native platform')
    require(core.sha(ROOT/'docs/architecture/native-checks.json') == raw['binding']['native_inventory_sha256'], 'native scope changed')
    require(core.sha(prepare) == raw['binding']['prepare']['sha256'],
            'resume preparation differs')
    for name in relevant_c46():
        require(core.sha(ROOT/name) == raw['helper_closure'].get(name),
                'completed C46 helper changed: ' + name)
    for name, digest in raw['artifact_sha256'].items():
        artifact = (ROOT/name).resolve()
        require(artifact.is_relative_to(path.parent) and core.sha(artifact) == digest,
                'prior immutable native artifact changed')
    require(work.parent == Path(tempfile.gettempdir()).resolve()
            and work.name.startswith('airs-p11-owned-') and work.is_dir()
            and not work.is_symlink() and work.stat().st_uid == os.getuid(),
            'resume work directory is not this user owned fixture')
    previous_state = path.parent/'fixture-state-final.json'
    require(raw['artifact_sha256'].get(str(previous_state.relative_to(ROOT))) == core.sha(previous_state),
            'prior final fixture state is not bound')
    state = json.loads(state_path.read_text())
    require(state == json.loads(previous_state.read_text()), 'fixture state changed since failed attempt')
    return raw, state


def restored_context(args):
    import fixture_app as fx
    import acceptance_c08 as c8
    prepared, bundle = core.verify_prepare(args.prepare.resolve())
    previous, state = inspect_previous(args.previous, args.prepare, args.work, Path(fx.DATA)/'state.json')
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
    if Path(c8.MGMT_ORIGIN_FILE).read_text().strip() != 'https://localhost:62780' or (Path(fx.DATA)/'fixture-academy-origin').read_text().strip() != 'https://localhost:62781':
        raise RuntimeError('retained management/Academy origins changed')
    ctx = core.Context.__new__(core.Context)  # no new scratch root or reset
    ctx.prepare_path, ctx.prepared, ctx.bundle = args.prepare.resolve(), prepared, bundle
    ctx.out = args.out.resolve(); ctx.out.mkdir(parents=True, exist_ok=False)
    ctx.work, ctx.log, ctx.rows, ctx.errors, ctx.artifacts = work, [], {}, [], {}
    ctx.fixture_root_owned = False
    ctx.argv = [sys.executable, *sys.argv]
    ctx.argv_source = 'sys.executable + sys.argv'
    ctx.started = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
    ctx.execution_exit_code = 1
    ctx.app = ctx.scan = ctx.mgmt = ctx.academy = None
    ctx.closure = core._binding.helper_closure(ROOT, 'acceptance_p11_resume')
    ctx.binding = dict(previous['binding'], helper_closure=ctx.closure,
                       conditions={'platform': platform.platform(), 'hostname': platform.node()})
    tenant = next(t for t in state['tenants'] if t['label'] == 'P11 Northwind')
    warning = next(w for group in state['tenantWarnings'] if group['tenant'] == tenant['id']
                   for w in group['warnings'] if w['product'] == 'modelSkill')
    ctx.state = {'bootstrap_tenant': 'P11 Northwind', 'spare_tenant': 'P11 Secondary',
                 'bootstrap_tenant_id': tenant['id'], 'original_supply_chain_warning': warning,
                 'scan_absent': True}
    ctx.continuation = {'observations': {'path': str(args.previous.resolve().relative_to(ROOT)),
                                         'sha256': core.sha(args.previous)},
                        'completed_ids': [C46], 'remaining_ids': sorted(set(core.inventory())-{C46}),
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
        api.run(ctx, prior_first_use={'observations': ctx.continuation['observations'],
            'evidence': {'path': str((args.previous.resolve().parent/'c48-first-use.json').relative_to(ROOT)),
                         'sha256': core.sha(args.previous.resolve().parent/'c48-first-use.json')}})
        settings.run(ctx)
        shell.run(ctx)
        setup.warning_flows(ctx)
        core.verify_prepare(ctx.prepare_path)
        ctx.require(core._binding.helper_closure(ROOT, 'acceptance_p11_resume') == ctx.closure,
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
    parser.add_argument('--previous', type=Path, required=True)
    parser.add_argument('--work', type=Path, required=True)
    parser.add_argument('--prepare', type=Path, required=True)
    parser.add_argument('--out', type=Path, required=True)
    raise SystemExit(execute(parser.parse_args()))
