"""Read the C44 gate's existing assertion map without changing its requirements."""
import json
import re
import runpy


def load_requirements(root):
    manifest = json.loads((root / 'docs/architecture/native-checks.json').read_text())['chunks']
    data = runpy.run_path(str(root / 'tools/native-acceptance/legacy_assertions.py'))
    assertions = data['ASSERTIONS']
    legacy_ids = {c['id'] for chunk, checks in manifest.items()
                  if re.fullmatch(r'C(0[1-9]|1\d|2\d|3[0-3])', chunk) for c in checks}
    assert legacy_ids == set(assertions), 'legacy map differs from the full manifest'
    targets = {t for kind, ts, _ in assertions.values()
               if kind in (data['X'], data['S'], data['P']) for t in ts}
    self_run = {lid.split('-')[0] for lid, (kind, _, _) in assertions.items() if kind == data['S']}
    required = {chunk: {t for t in targets if t.startswith(chunk + '-')} for chunk in self_run}
    # Some legacy ids are carried by a different self-run driver. Only drivers with actual targets run;
    # the union check below still requires every mapped target to be covered.
    required = {chunk: checks for chunk, checks in required.items() if checks}
    for chunk in [f'C{i}' for i in range(35, 45)]:
        required[chunk] = {c['id'] for c in manifest[chunk]}
    assert all(required.values()), 'empty required native check set'
    assert targets <= set().union(*required.values()), 'mapped native target has no driver'
    return required


def assess_record(record, root, executable_sha256, required):
    errors = []
    if record.get('app', {}).get('executable_sha256') != executable_sha256:
        errors.append('stale executable')
    checks = {c['id']: c for c in record.get('checks', [])}
    for cid in sorted(required):
        check = checks.get(cid)
        if check is None:
            errors.append(cid + ' missing')
        elif check['result'] != 'pass':
            errors.append(cid + ' ' + check['result'])
        else:
            errors.extend(cid + ' missing evidence: ' + e for e in check['evidence'] if not (root / e).exists())
    carried_elsewhere = [cid for cid, c in checks.items() if cid not in required and c['result'] != 'pass']
    return errors, carried_elsewhere
