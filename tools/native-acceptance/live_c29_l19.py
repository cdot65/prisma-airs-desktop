#!/usr/bin/env python3
"""One approved C29 wizard verify. Run from the granted native queue, never
against a synthetic-keychain fixture bundle. No automatic live retry.

The qualification bundle uses production adapters with a separate data identity;
the exact build, source, flags and executable are recorded by its build receipt.
Only a locally stored placeholder scan key and the approved management client
are entered. Only Runtime Security is selected, and onboarding is not completed.
"""
import argparse
import datetime
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import time

sys.path.insert(0, str(Path(__file__).resolve().parent))
import fixture_app as fx

IDENTITY = 'io.cdot.prisma-airs-desktop.v2.l19qual'
SERVICE = 'io.cdot.prisma-airs-desktop.v2.scan'
LIVE_REF = ('io.cdot.prisma-airs-desktop.v2.live', 'mgmt-vulture-client-secret')
OPERATION = 'one management credential verification through the first-run wizard (C29): one latest security-profile probe with limit=1; discard profile contents; no other inventory browsing or writes'
ROOT = Path.cwd().resolve()
DATA = Path.home() / 'Library/Application Support' / IDENTITY
EXE = None


def sha(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + '\n')


class PrivateApp(fx.App):
    @staticmethod
    def running_pid():
        rows = fx.sh(['ps', '-axo', 'pid=,comm=']).stdout.splitlines()
        ids = [int(parts[0]) for row in rows if len(parts := row.strip().split(None, 1)) == 2 and parts[1] == str(EXE)]
        if len(ids) > 1:
            raise RuntimeError('multiple qualification processes')
        return ids[0] if ids else None

    def quit(self):
        pid = self.running_pid()
        if pid:
            os.kill(pid, signal.SIGTERM)
            for _ in range(40):
                if not self.running_pid():
                    break
                time.sleep(.25)
            if self.running_pid():
                os.kill(pid, signal.SIGKILL)
                time.sleep(.5)
            if self.running_pid():
                raise RuntimeError('qualification process did not stop')
        self.pid = None

    def ax(self, command, *args):
        # No automatic diagnostic tree during private entry or on failure.
        out = fx.sh([self.ax_bin, command, str(self.pid), *map(str, args)])
        if out.returncode:
            raise RuntimeError(f'accessibility {command} failed')
        return json.loads(out.stdout or '[]') if command == 'tree' else out.stdout

    def private_type(self, label, value, helper):
        self.ax('focus', 'AXTextField', label, 0)
        self.front()
        result = subprocess.run([str(helper), str(self.pid)], input=value, text=True, capture_output=True)
        if result.returncode:
            raise RuntimeError('private credential entry failed')
        time.sleep(.5)
        field = self.field('AXTextField', label) or {}
        arrived = str(field.get('value') or '')
        if len(arrived) != len(value) or not set(arrived) <= {'•'}:
            raise RuntimeError('private field did not report a masked value')


def require(app, predicate, message, timeout=20):
    tree, ok = app.wait_for(predicate, timeout)
    if not ok:
        raise RuntimeError(message)
    return tree


def press(app, label):
    require(app, lambda t: app.node(t, 'AXButton', label) is not None, f'{label} unavailable')
    app.ax('press', 'AXButton', label)
    time.sleep(.5)


def verify_finished(tree):
    terminal = {
        'Checks finished; nothing failed.',
        'Checks finished with warnings.',
        'A required check failed.',
        'An optional check failed. You can fix it and verify again, or continue with it recorded as a warning.',
    }
    labels = {n.get('label') for n in tree if n.get('role') in ('AXStaticText', 'AXHeading')}
    return bool(labels & terminal) and 'Checking…' not in labels


def reserve_read(path, manifest_sha, attempt, limit):
    """Append a conservative reservation before admission; never refund one."""
    path.parent.mkdir(parents=True, exist_ok=True)
    fd = os.open(path, os.O_RDWR | os.O_CREAT, 0o600)
    with os.fdopen(fd, 'a+', encoding='utf-8') as ledger:
        fcntl.flock(ledger, fcntl.LOCK_EX)
        assert os.fstat(ledger.fileno()).st_mode & 0o077 == 0, 'budget ledger must be private'
        ledger.seek(0)
        raw = ledger.read()
        entries = [json.loads(line) for line in raw.splitlines() if line.strip()]
        assert all(e['manifest_sha256'] == manifest_sha for e in entries), 'budget manifest changed'
        assert not any(e['attempt'] == attempt for e in entries), 'attempt already reserved'
        used = sum(e['reads_reserved'] for e in entries)
        assert all(e['reads_reserved'] == 1 for e in entries) and used < limit <= 2, 'read budget exhausted'
        ledger.seek(0, os.SEEK_END)
        if raw and not raw.endswith('\n'):
            ledger.write('\n')
        ledger.write(json.dumps({'manifest_sha256': manifest_sha, 'attempt': attempt, 'reads_reserved': 1}) + '\n')
        ledger.flush()
        os.fsync(ledger.fileno())
    return used + 1


def credential_ids():
    path = DATA / 'state.json'
    if not path.exists():
        return set()
    state = json.loads(path.read_text())
    return {c['record'] for c in state.get('credentials', [])} | set(state.get('pendingCleanup', []))


def main():
    global EXE
    parser = argparse.ArgumentParser()
    parser.add_argument('--scope', required=True, type=Path)
    parser.add_argument('--build', required=True, type=Path)
    parser.add_argument('--validated', required=True, type=Path)
    parser.add_argument('--budget-ledger', required=True, type=Path)
    parser.add_argument('--attempt', required=True)
    args = parser.parse_args()
    assert re.fullmatch(r'attempt-\d{2}', args.attempt)
    scope_bytes = args.scope.read_bytes()
    scope_sha = hashlib.sha256(scope_bytes).hexdigest()
    scope = json.loads(scope_bytes)
    validated = json.loads(args.validated.read_text())
    assert validated['manifest_sha256'] == scope_sha and validated['valid'] is True
    family = scope['families']['L19']
    assert family['status'] == 'designated' and family['environment'] == 'test-b'
    assert family['permitted_operations'] == [OPERATION]
    assert family['budget'] in ({'reads': 1}, {'reads': 2})
    assert family['nonsecret']['tsg_id'] == '1852583913' and family['nonsecret']['region'] == 'us'
    ref = family['credentials']['client_secret']
    assert ref['kind'] == 'keychain' and (ref['service'], ref['account']) == LIVE_REF
    build = json.loads(args.build.read_text())
    assert Path(build['source_root']).resolve() == ROOT
    assert build['identifier'] == IDENTITY and build['features'] == []
    inventory = ROOT / build['source_inventory']
    assert sha(inventory) == build['source_inputs_sha256']
    for line in inventory.read_text().splitlines():
        expected, relative = line.split(None, 1)
        assert sha(ROOT / relative.lstrip(' *')) == expected, 'qualification source changed after build'
    bundle = Path(build['bundle'])
    EXE = bundle / 'Contents/MacOS/prisma-airs-desktop-v2'
    assert sha(EXE) == build['executable_sha256']
    actual_id = fx.sh(['plutil', '-extract', 'CFBundleIdentifier', 'raw', '-o', '-', str(bundle / 'Contents/Info.plist')]).stdout.strip()
    assert actual_id == IDENTITY
    assert not DATA.exists(), 'qualification data root must be fresh'
    out = ROOT / 'docs/planning/evidence/C29/live/L19' / args.attempt
    assert not out.exists(), 'never overwrite an earlier live attempt'
    out.mkdir(parents=True)
    work = Path(tempfile.mkdtemp(prefix='airs-l19-'))
    work.chmod(0o700)
    helper = work / 'private-input'
    compiled = fx.sh(['swiftc', '-O', str(Path(__file__).with_name('type_private_stdin.swift')), '-o', str(helper)])
    assert compiled.returncode == 0, 'private input helper failed to compile'
    app = PrivateApp(str(bundle), str(work), [])
    receipt = {'schema': 'airs-v2/live-row/1', 'subject': 'C29', 'family': 'L19',
               'performed_by': 'Codex through the granted Kitty native queue',
               'attempt': args.attempt, 'status': 'failed', 'recorded_utc': datetime.datetime.now(datetime.timezone.utc).isoformat(),
               'manifest_sha256': scope_sha, 'build_receipt_sha256': sha(args.build),
               'executable_sha256': build['executable_sha256'], 'source_inputs_sha256': build['source_inputs_sha256'],
               'client_id': family['nonsecret']['client_id'], 'tsg_id': '1852583913',
               'operation': OPERATION, 'wizard_verify_intents': 0, 'read_budget_reserved': 0,
               'http_requests_observed': False, 'automatic_retries': 0, 'products': ['runtimeSecurity'],
               'evidence': [], 'local_credentials_removed': False,
               'limitations': ['Request bound comes from the inspected production verifier (limit=1, latest=true, SDK retries=0) and exactly one wizard intent; no TLS interception or response body capture.']}
    write(out / 'build.json', build)
    shutil.copy2(inventory, out / 'source-inputs.sha256')
    ids = set()
    secret = ''
    stage = 'launch'
    try:
        app.launch()
        stage = 'create draft'
        press(app, 'Set up a tenant')
        app.type_into('AXTextField', 'Display name', 'Vulture L19 qualification')
        app.type_into('AXTextField', 'TSG ID', '1852583913')
        app.ax('press', 'AXRadioButton', 'Other')
        press(app, 'Create tenant')
        require(app, lambda t: app.node(t, 'AXButton', 'Save scan key') is not None, 'draft unavailable')
        app.private_type('Scan API key', 'l19-local-placeholder-never-sent', helper)
        press(app, 'Save scan key')
        require(app, lambda t: app.node(t, 'AXButton', 'Replace scan key') is not None, 'scan placeholder not stored')
        ids |= credential_ids()
        stage = 'store management client'
        app.type_into('AXTextField', 'Client ID', family['nonsecret']['client_id'])
        resolved = subprocess.run(['security', 'find-generic-password', '-w', '-s', ref['service'], '-a', ref['account']], capture_output=True)
        if resolved.returncode:
            raise RuntimeError('approved Keychain reference unavailable')
        secret = resolved.stdout.decode().rstrip('\n')
        resolved.stdout = b''
        if not secret or len(secret) > 4096:
            raise RuntimeError('approved Keychain value unusable')
        app.private_type('Client secret', secret, helper)
        press(app, 'Save management client')
        require(app, lambda t: app.node(t, 'AXButton', 'Replace management client') is not None, 'management client not stored')
        ids |= credential_ids()
        assert len(ids) == 2, 'the fresh draft must own exactly its two local credentials'
        stage = 'restrict products'
        press(app, 'Continue')
        tree = require(app, lambda t: app.node(t, 'AXHeading', 'Which products') is not None, 'products unavailable')
        for label in ('AI Gateway', 'AI Red Teaming', 'Model and skill scans'):
            n = app.node(tree, 'AXCheckBox', label)
            if n and n.get('value') in (1, '1', True):
                app.ax('press', 'AXCheckBox', label)
                time.sleep(.5)
                tree = app.tree()
        assert not any(n.get('role') == 'AXCheckBox' and n.get('value') in (1, '1', True) and n.get('label') != 'AI Runtime Security' for n in tree)
        press(app, 'Continue')
        require(app, lambda t: app.node(t, 'AXHeading', 'Check the connection') is not None, 'verify unavailable')
        # Charge before the only network-triggering action, including interrupted
        # or uncertain earlier calls. A third reservation is refused locally.
        receipt['cumulative_read_budget_reserved'] = reserve_read(args.budget_ledger, scope_sha, args.attempt, family['budget']['reads'])
        receipt['read_budget_reserved'] = 1
        receipt['wizard_verify_intents'] = 1
        stage = 'verify'
        press(app, 'Run the checks')
        tree = require(app, verify_finished, 'verify did not settle', 90)
        passed = app.has_text(tree, 'The management client can read this tenant.') and app.has_text(tree, 'Pass')
        assert all(secret not in str(n) for n in tree), 'sensitive tree refused'
        write(out / 'wizard-verify.json', tree)
        receipt['evidence'].append(str((out / 'wizard-verify.json').relative_to(ROOT)))
        receipt['status'] = 'pass' if passed else 'failed'
        receipt['verification'] = 'management client passed' if passed else 'management verification failed'
        stage = 'remove local credentials'
        # Stop at Verify. Home/Inbox and optional product listings are never opened.
        press(app, 'Tenant')
        for label, gone in [('Remove management client', 'Replace management client'), ('Remove scan key', 'Replace scan key')]:
            press(app, label)
            require(app, lambda t, gone=gone: app.node(t, 'AXButton', gone) is None, 'local credential removal not confirmed')
    except Exception as error:
        # Fixed messages from this script only; never echo Keychain output,
        # credential-bearing subprocess arguments, an AX tree or a traceback.
        receipt['failure_type'] = type(error).__name__
        receipt['failure_stage'] = stage
    finally:
        ids |= credential_ids()
        app.quit()
        removed = True
        for record in ids:
            if not re.fullmatch(r'cr-[0-9a-f]{16}', record):
                removed = False
                continue
            exists = fx.sh(['security', 'find-generic-password', '-s', SERVICE, '-a', record])
            if exists.returncode == 0:
                deleted = fx.sh(['security', 'delete-generic-password', '-s', SERVICE, '-a', record])
                removed = removed and deleted.returncode == 0
            missing = fx.sh(['security', 'find-generic-password', '-s', SERVICE, '-a', record])
            removed = removed and missing.returncode == 44
        receipt['local_credentials_removed'] = removed
        receipt['local_record_count'] = len(ids)
        receipt['events'] = app.log
        if not removed:
            receipt['status'] = 'failed'
        write(out / 'receipt.json', receipt)
        secret = ''
        if removed and DATA.exists():
            shutil.rmtree(DATA)
        shutil.rmtree(work)
    print(json.dumps({'family': 'L19', 'status': receipt['status'], 'local_credentials_removed': receipt['local_credentials_removed'], 'receipt': str(out / 'receipt.json')}))
    return 0 if receipt['status'] == 'pass' else 1


if __name__ == '__main__':
    sys.exit(main())
