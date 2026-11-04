#!/usr/bin/env python3
"""Execute only the two still-uncompleted controlled warning assertions."""
import argparse
import os
from pathlib import Path

import acceptance_p11 as core
import acceptance_p11_verify as previous_driver
import acceptance_p11_warning_text as warning_text
import fixture_app as fx

ROOT = core.ROOT


def execute(args):
    if not os.environ.get('AIRS_NATIVE_ORCHESTRATOR'):
        raise RuntimeError('set AIRS_NATIVE_ORCHESTRATOR to the actual VM UI operator')
    ctx = previous_driver.restored_context(args)
    ctx.closure = core._binding.helper_closure(ROOT, 'acceptance_p11_warn')
    ctx.binding['helper_closure'] = ctx.closure
    ctx.require(set(ctx.continuation['remaining_ids']) == {'C46-N02', 'C46-N03'},
                'warning slice requires exactly the two remaining assertions')
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
            offsets[name] = len(endpoint.requests()); endpoint.start()
        ctx.save('endpoint-history-offsets', offsets)
        ctx.app.launch()
        warning_text.run(ctx)
        core.verify_prepare(ctx.prepare_path)
        ctx.require(core._binding.helper_closure(ROOT, 'acceptance_p11_warn') == ctx.closure,
                    'warning helper changed during execution')
        ctx.require(set(ctx.rows) == {'C46-N02', 'C46-N03'}
                    and all(value is True for row in ctx.rows.values() for value in row['facts'].values()),
                    'warning facts remain incomplete')
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
    for name in ('work', 'prepare', 'out'):
        parser.add_argument('--'+name, type=Path, required=True)
    raise SystemExit(execute(parser.parse_args()))
