"""Independent completeness check for the retained C29 native assertions.

The expected axes come from C29-N06/T09, rather than the rows a driver happens
to emit. Missing behavior or a missing capture cannot be reported as exercised.
"""
import argparse
import hashlib
import itertools
import json
from pathlib import Path

STEPS = {
    'firstRun': ['welcome', 'tenant', 'products', 'verify', 'packs', 'academy', 'done'],
    'add': ['tenant', 'products', 'verify', 'packs', 'done'],
}
BEHAVIOR = [
    'welcome_relaunch', 'profile_edit_saved', 'profile_edit_key_kept',
    'gateway_listed', 'workspace_chosen', 'workspace_key_saved',
    'rejected_client_failed', 'failed_client_continuable', 'failed_scan_stored_not_pass',
    'credential_fix_returns_tenant', 'correct_client_saved', 'workspace_rechosen_after_fix',
    'verification_passed', 'gateway_verification_passed', 'product_edit_marked_stale',
    'stale_continuation_closed', 'stale_rerun_clean', 'home_relaunch',
    'starter_cases_imported', 'secret_confinement',
    'keyed_draft_saved', 'cancelled_rows_unchanged', 'cancelled_active_unchanged', 'cancelled_key_deleted',
]


def problems(observations, root):
    coverage = observations.get('c29_native_coverage', {})
    behavior = coverage.get('behavior', {})
    failures = ['missing/failed retained behavior: ' + key for key in BEHAVIOR if behavior.get(key) is not True]
    rows = coverage.get('layouts', {})
    expected = {(mode, step, theme, zoom) for mode, steps in STEPS.items()
                for step, theme, zoom in itertools.product(steps, ['Light', 'Dark'], [1, 2])}
    seen = set()
    for name, row in rows.items():
        axis = (row.get('mode'), row.get('step'), row.get('theme'), row.get('zoom'))
        if axis in seen:
            failures.append('duplicate matrix row: ' + str(axis))
        seen.add(axis)
        zoom = row.get('zoom')
        requested = (1440, 900) if zoom == 1 else (900, 600)
        geometry = row.get('geometry', {})
        web = geometry.get('web') or []
        viewport = geometry.get('viewport') or web
        if not (len(web) == 2 and len(viewport) == 2 and web[0] == requested[0]
                and viewport[0] == requested[0] and (zoom == 1 or viewport[1] == requested[1])):
            failures.append(name + ': wrong measured geometry')
        primary = row.get('primary', {})
        height = (primary.get('size') or [0, 0])[1]
        if not (primary.get('inside') and zoom in (1, 2) and height >= 32 * zoom):
            failures.append(name + ': primary outside window or too small')
        if row.get('overflow_px') != 0 or row.get('horizontal_scrollbars') != 0:
            failures.append(name + ': horizontal overflow')
        if row.get('dialog') != (row.get('mode') == 'add'):
            failures.append(name + ': wrong modal ownership')
        keyboard = row.get('keyboard', {})
        if not (keyboard.get('primary_reached') and keyboard.get('walk') and keyboard.get('contained')):
            failures.append(name + ': keyboard traversal missing/incomplete')
        for field in ['capture', 'tree']:
            path = Path(root) / row.get(field, '')
            if not row.get(field) or not path.is_file():
                failures.append(name + ': missing ' + field)
            elif hashlib.sha256(path.read_bytes()).hexdigest() != row.get(field + '_sha256'):
                failures.append(name + ': hash mismatch for ' + field)
    failures.extend('missing matrix row: ' + str(axis) for axis in sorted(expected - seen))
    failures.extend('unexpected matrix row: ' + str(axis) for axis in sorted(seen - expected))
    return failures


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('observations', type=Path)
    parser.add_argument('--root', type=Path, default=Path.cwd())
    args = parser.parse_args()
    data = json.loads(args.observations.read_text())
    errors = problems(data['observations'], args.root)
    print(json.dumps({'retained_C29_native_coverage': 'fail' if errors else 'pass', 'errors': errors}, indent=2))
    return 1 if errors else 0


if __name__ == '__main__':
    raise SystemExit(main())
