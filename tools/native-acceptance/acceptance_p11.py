#!/usr/bin/env python3
"""Affected P11 native flow, with immutable raw observations and separate visual review.

Execute only against --prepare's exact fixture bundle. --finalize performs no UI:
it binds measured facts, actual P11 test records, and capture-specific visual review.
No old scan-only setup helper, installed app lookup, or process-name signaling.
"""
import argparse
import hashlib
import importlib.util
import json
import os
from pathlib import Path
import platform
import plistlib
import re
import sys
import tempfile
import time
from typing import TYPE_CHECKING

# Literal subprocess server inputs must join candidate.helper_closure's AST graph.
if TYPE_CHECKING:
    import mgmt_server, scan_server, academy_server, academy_catalog

ROOT = Path.cwd().resolve()
TOOLS = Path(__file__).resolve().parent
sys.path.insert(0, str(TOOLS))
_spec = importlib.util.spec_from_file_location('p11_candidate_bindings', ROOT/'tools/p10/candidate.py')
_binding = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(_binding)


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write_new(path, value):
    path = Path(path)
    with path.open('x') as stream:
        json.dump(value, stream, indent=2)
        stream.write('\n')


def inventory():
    chunks = json.loads((ROOT/'docs/architecture/native-checks.json').read_text())['chunks']
    return {row['id']: row['check'] for chunk in ('C46', 'C47', 'C48', 'C49') for row in chunks[chunk]}


def verify_prepare(path):
    prepared = json.loads(path.read_text())
    bundle = (ROOT/prepared['bundle']).resolve(strict=True)
    info = plistlib.loads((bundle/'Contents/Info.plist').read_bytes())
    if prepared['identifier'] != 'io.cdot.airs-desktop.fixture' or info.get('CFBundleIdentifier') != prepared['identifier']:
        raise RuntimeError('only the explicitly prepared fixture identity is allowed')
    if info.get('CFBundleExecutable') != 'prisma-airs-desktop':
        raise RuntimeError('unexpected fixture executable')
    if sha(bundle/'Contents/MacOS/prisma-airs-desktop') != prepared['executable_sha256']:
        raise RuntimeError('prepared executable drift')
    if _binding.bundle_hash(bundle) != prepared['bundle_sha256']:
        raise RuntimeError('complete prepared bundle drift')
    return prepared, bundle


class Context:
    def __init__(self, prepare, out):
        self.prepare_path, self.out = prepare.resolve(), out.resolve()
        self.prepared, self.bundle = verify_prepare(self.prepare_path)
        self.out.mkdir(parents=True, exist_ok=False)
        self.work = Path(tempfile.mkdtemp(prefix='airs-p11-owned-'))
        self.log, self.rows, self.errors, self.artifacts = [], {}, [], {}
        self.fixture_root_owned = False
        self.argv = list(getattr(sys, "orig_argv", [sys.executable, *sys.argv]))
        self.state = {'bootstrap_tenant': 'P11 Northwind', 'spare_tenant': 'P11 Secondary', 'scan_absent': True}
        self.closure = _binding.helper_closure(ROOT, 'acceptance_p11')
        self.binding = {'prepare': {'path': str(self.prepare_path), 'sha256': sha(self.prepare_path)},
                        'bundle': str(self.bundle), 'executable_sha256': self.prepared['executable_sha256'],
                        'bundle_sha256': self.prepared['bundle_sha256'], 'helper_closure': self.closure,
                        'native_inventory_sha256': sha(ROOT/'docs/architecture/native-checks.json')}
        self.started = time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime())
        self.app = self.mgmt = self.scan = self.academy = None

    @staticmethod
    def require(condition, message):
        if not condition:
            raise RuntimeError(message)

    def relative(self, path):
        return str(Path(path).resolve().relative_to(ROOT))

    def save(self, name, tree=None):
        self.require(re.fullmatch(r'[A-Za-z0-9_.-]+', name), 'unsafe evidence name')
        path = self.out/(name+'.json')
        write_new(path, self.app.tree() if tree is None else tree)
        relative = self.relative(path)
        self.artifacts[relative] = sha(path)
        return relative

    def capture(self, name):
        import acceptance_c29 as c29
        self.require(not (self.out/'captures'/(name+'.png')).exists(), 'capture already exists')
        c29.NATIVE = str(self.out)
        path = c29.capture(self.app, name)
        self.require(path, 'fixture window did not produce a capture')
        self.artifacts[path] = sha(ROOT/path)
        return path

    def press(self, role, label, timeout=12):
        tree, ready = self.app.wait_for(lambda t: self.app.node(t, role, label) is not None, timeout)
        self.require(ready, f'missing {role} {label}')
        node = self.app.node(tree, role, label)
        self.require(node.get('enabled') is not False, f'disabled {role} {label}')
        self.app.ax('press', role, label)
        time.sleep(.3)
        return self.app.tree()

    def press_any(self, label, timeout=12):
        roles = ('AXButton', 'AXCheckBox', 'AXRadioButton', 'AXPopUpButton')
        tree, ready = self.app.wait_for(lambda t: any(self.app.node(t, r, label) for r in roles), timeout)
        self.require(ready, f'missing control {label}')
        role = next(r for r in roles if self.app.node(tree, r, label))
        return self.press(role, label, timeout)

    def text(self, fragment, timeout=15):
        tree, ready = self.app.wait_for(lambda t: self.app.has_text(t, fragment), timeout)
        self.require(ready, f'missing fixture text {fragment}')
        return tree

    def heading(self, label, timeout=15):
        tree, ready = self.app.wait_for(lambda t: self.app.node(t, 'AXHeading', label) is not None, timeout)
        self.require(ready, f'missing heading {label}')
        return tree

    def navigate(self, label):
        self.app.menu('Go', label)
        return self.heading(label)

    def fixture_state(self):
        import fixture_app as fx
        return json.loads((Path(fx.DATA)/'state.json').read_text())

    def wait_state(self, predicate, message, timeout=15):
        deadline = time.monotonic()+timeout
        while time.monotonic() < deadline:
            value = self.fixture_state()
            if predicate(value):
                return value
            time.sleep(.2)
        raise RuntimeError(message)

    def record(self, check_id, facts, evidence, visual=None):
        self.require(check_id in inventory() and check_id not in self.rows, 'unknown or repeated assertion')
        self.require(bool(facts) and all(type(value) is bool for value in facts.values()), 'facts must be measured booleans')
        for path in evidence + list(visual or []):
            self.require((ROOT/path).resolve().is_relative_to(self.out) and (ROOT/path).is_file(), 'evidence must be inside this immutable attempt')
            self.artifacts[path] = sha(ROOT/path)
        self.rows[check_id] = {'id': check_id, 'facts': facts, 'evidence': evidence, 'visual': list(visual or [])}
        self.partial()

    def partial(self):
        # This file is progress, never acceptance; final raw record uses exclusive creation.
        (self.out/'observations.partial.json').write_text(json.dumps(self.observations(False), indent=2)+'\n')

    def observations(self, complete):
        return {'schema': 'airs-desktop/p11-native-observations/1', 'subject': 'P11', 'complete': complete,
                'argv': self.argv, 'started_utc': self.started, 'finished_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                'performed_by': os.environ.get('AIRS_NATIVE_ORCHESTRATOR'), 'host': platform.platform(),
                'execution_id': self.out.name, 'status': 'observed' if complete else 'failed', 'exit_code': 0 if complete else 1,
                'executable_sha256': self.prepared['executable_sha256'], 'bundle_sha256': self.prepared['bundle_sha256'], 'helper_closure': self.closure,
                'binding': self.binding, 'artifact_sha256': self.artifacts, 'checks': list(self.rows.values()), 'errors': self.errors, 'log': self.log,
                'meaning': 'Actual fixture AX/keyboard/geometry observations. Pixels and production-interface results need separate bound review; no unrun assertion is a pass.'}


def execute(args):
    import fixture_app as fx
    import acceptance_c08 as c8
    import academy_catalog
    import acceptance_p11_setup as setup
    import acceptance_p11_api as api
    import acceptance_p11_settings as settings
    import acceptance_p11_shell as shell
    if not os.environ.get('AIRS_NATIVE_ORCHESTRATOR'):
        raise RuntimeError('set AIRS_NATIVE_ORCHESTRATOR to the actual UI operator')
    ctx = Context(args.prepare, args.out)
    os.environ['AIRS_NATIVE_PROCESS_RECORD'] = str(ctx.out/'candidate-processes.json')
    try:
        ctx.app = fx.App(str(ctx.bundle), str(ctx.work), ctx.log)
        candidate = ctx.app.candidate
        ctx.require(not candidate.matching(candidate.app_path) and not candidate.matching(candidate.engine_path), 'unowned prepared fixture process exists; refusing data cleanup or adoption')
        ctx.fixture_root_owned = True
        fx.clean_fixture_root()
        ctx.scan = fx.ScanEndpoint(str(ctx.work/'scan'), c8.SCAN_KEY)
        ctx.scan.install_roots()
        ctx.scan.start()
        ctx.mgmt = c8.MgmtEndpoint(str(ctx.work/'scan'), str(ctx.work/'mgmt'))
        ctx.mgmt.install()
        ctx.mgmt.start()
        ctx.academy = fx.AcademyEndpoint(str(ctx.work/'academy'))
        (Path(ctx.academy.dir)/'catalog.json').write_text(json.dumps(academy_catalog.build(str(ROOT))))
        ctx.academy.install()
        ctx.academy.start()
        # Both trust anchors are generated by this attempt, never system/login roots.
        Path(fx.ROOTS).write_bytes((ctx.work/'scan/ca.pem').read_bytes()+b'\n'+(ctx.work/'academy/ca.pem').read_bytes())
        setup.bootstrap(ctx)
        api.run(ctx)  # first API entry must occur here, before shell page matrix
        settings.run(ctx)
        shell.run(ctx)
        setup.warning_flows(ctx)
        verify_prepare(ctx.prepare_path)
        ctx.require(_binding.helper_closure(ROOT, 'acceptance_p11') == ctx.closure, 'native helper changed during execution')
        ctx.require(set(ctx.rows) == set(inventory()), 'some native assertions were never reached')
        ctx.require(all(value for row in ctx.rows.values() for value in row['facts'].values()), 'one or more measured native facts failed')
    except Exception as error:
        ctx.errors.append({'type': type(error).__name__, 'message': str(error)})
        if ctx.app:
            try:
                ctx.save('failure-tree')
            except Exception:
                pass
    finally:
        if ctx.fixture_root_owned:
            for name, endpoint in (("management", ctx.mgmt), ("scan", ctx.scan), ("academy", ctx.academy)):
                if endpoint:
                    try:
                        ctx.save(name+"-requests", endpoint.requests())
                    except Exception as error:
                        ctx.errors.append({'type': type(error).__name__, 'message': 'fixture request capture: '+str(error)})
            try:
                fx.stop_app()
                fx.stop_engines()
                ctx.save('fixture-state-final', ctx.fixture_state())
            except Exception as error:
                ctx.errors.append({'type': type(error).__name__, 'message': 'owned teardown: '+str(error)})
        for endpoint in (ctx.academy, ctx.mgmt, ctx.scan):
            if endpoint:
                endpoint.stop()
        # Preserve failed owned fixture state for diagnosis; never copy a Keychain.
        if ctx.fixture_root_owned and not ctx.errors:
            fx.clean_fixture_root()
        write_new(ctx.out/'observations.json', ctx.observations(not ctx.errors))
    print('P11 observations saved; acceptance requires bound test and pixel review.')
    return 1 if ctx.errors else 0


def test_run(path):
    value = json.loads(path.read_text())
    if not (value['schema'] == 'airs-v2/run/1' and value['subject'] == 'P11' and value['kind'] == 'test'
            and value['status'] == 'passed' and value['failure'] is None and value['inputs_before'] == value['inputs_after']):
        raise RuntimeError('actual passing stable-input P11 test run is required')
    commands = value['commands']
    if not commands or any(c['status'] != 'passed' or c['exit_code'] != 0 for c in commands):
        raise RuntimeError('test command missing or failed')
    for command in commands:
        log = ROOT/command['log']
        if sha(log) != command['output_sha256']:
            raise RuntimeError('test raw log drift')
    tests = [c['tests'] for c in commands if c.get('tests')]
    if not tests or any(t['failed'] for t in tests) or sum(t['passed'] for t in tests) == 0:
        raise RuntimeError('no actual passing tests')
    argv = '\n'.join(' '.join(c['argv']) for c in commands)
    if 'setup_warning_recheck' not in argv or 'shell-refinement.test.tsx' not in argv:
        raise RuntimeError('required warning/shell boundary membership absent')
    return {'path': str(path), 'sha256': sha(path), 'inputs': value['inputs_after']}


def finalize(args):
    raw_path = args.finalize.resolve()
    raw = json.loads(raw_path.read_text())
    if raw.get('schema') != 'airs-desktop/p11-native-observations/1' or not raw.get('complete') or raw.get('errors') or raw.get('status') != 'observed' or raw.get('exit_code') != 0:
        raise RuntimeError('incomplete or failed native observations cannot qualify')
    binding = raw['binding']
    prepare_path = Path(binding['prepare']['path'])
    prepared, _ = verify_prepare(prepare_path)
    if sha(ROOT/'docs/architecture/native-checks.json') != binding['native_inventory_sha256']:
        raise RuntimeError('native assertion inventory drift')
    for artifact, expected in raw['artifact_sha256'].items():
        if not (ROOT/artifact).resolve().is_relative_to(raw_path.parent) or sha(ROOT/artifact) != expected:
            raise RuntimeError('raw native artifact changed')
    if sha(prepare_path) != binding['prepare']['sha256'] or _binding.helper_closure(ROOT, 'acceptance_p11') != binding['helper_closure']:
        raise RuntimeError('prepare/helper binding changed')
    test = test_run(args.test_run.resolve())
    review = json.loads(args.visual_review.read_text())
    if not (review['schema'] == 'airs-desktop/p11-visual-review/1' and review['observations_sha256'] == sha(raw_path)
            and review['executable_sha256'] == prepared['executable_sha256'] and review['bundle_sha256'] == prepared['bundle_sha256']
            and isinstance(review.get('performed_by'), str) and review['performed_by']):
        raise RuntimeError('visual review does not bind the actual observations/candidate')
    images = {(r['path'], r['assertion']): r for r in review['captures']}
    checks = []
    rows = {r['id']: r for r in raw['checks']}
    if len(rows) != len(raw['checks']) or set(rows) != set(inventory()):
        raise RuntimeError('native assertion membership differs')
    for check_id, description in inventory().items():
        row = rows[check_id]
        if not row['facts'] or any(value is not True for value in row['facts'].values()):
            raise RuntimeError('measured fact did not pass: '+check_id)
        for image in row['visual']:
            path = (ROOT/image).resolve()
            if not path.is_relative_to(raw_path.parent) or not path.is_file():
                raise RuntimeError('capture outside the actual attempt')
            judged = images.get((image, check_id))
            if not judged or judged.get('accepted') is not True or judged.get('sha256') != sha(path) or judged.get('findings'):
                raise RuntimeError('missing, failed or drifted pixel review: '+check_id+' '+image)
        checks.append({'id': check_id, 'description': description, 'result': 'pass',
                       'observation': 'Measured native facts: '+', '.join(row['facts'])+'. Pixels independently inspected where listed; complex stale/rollback/race boundaries bind the actual P11 test run.',
                       'evidence': [str(raw_path.relative_to(ROOT)), str(args.visual_review.resolve().relative_to(ROOT)), str(args.test_run.resolve().relative_to(ROOT)), *row['evidence'], *row['visual']]})
    record = {'schema': 'airs-v2/native-acceptance/1', 'subject': 'P11', 'recorded_utc': time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
              'method': 'cua', 'performed_by': raw['performed_by'], 'host': raw['host'],
              'app': {'bundle': prepared['bundle'], 'identifier': prepared['identifier'], 'executable_sha256': prepared['executable_sha256'], 'fixture': True},
              'checks': checks, 'limitations': ['Synthetic fixture only; no installed/live acceptance, production app control, user data, login Keychain, real credentials or product configuration changes. Linux qualification is separate. Raw outcomes remain unchanged.']}
    write_new(raw_path.parent/'native-acceptance.json', record)
    write_new(raw_path.parent/'native-bindings.json', {'schema': 'airs-desktop/p11-native-bindings/1', 'observations': {'path': str(raw_path), 'sha256': sha(raw_path)}, 'visual_review': {'path': str(args.visual_review), 'sha256': sha(args.visual_review)}, 'test_run': test, **binding})
    return 0


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prepare', type=Path, default=ROOT/'docs/planning/evidence/P11/native-prepare.json')
    parser.add_argument('--out', type=Path, default=ROOT/'docs/planning/evidence/P11/native-attempt-01')
    parser.add_argument('--finalize', type=Path)
    parser.add_argument('--visual-review', type=Path)
    parser.add_argument('--test-run', type=Path)
    args = parser.parse_args()
    if args.finalize:
        if not args.visual_review or not args.test_run:
            parser.error('--finalize requires --visual-review and actual --test-run')
        return finalize(args)
    return execute(args)


if __name__ == '__main__':
    raise SystemExit(main())
