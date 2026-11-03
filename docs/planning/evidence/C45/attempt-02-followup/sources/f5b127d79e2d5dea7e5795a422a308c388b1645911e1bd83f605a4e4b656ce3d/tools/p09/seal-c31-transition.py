"""Seal the exact authorized C31 delta after focused preflight, before build.

No qualification result or review score is created here. Pass explicit fix
commits and actual passing preflight JSON records with command/exit fields.
"""
from pathlib import Path
import argparse
import json
import subprocess
import datetime
import sys
sys.path.insert(0, str(Path(__file__).resolve().parent))
import c31_transition_policy as policy
from phase_packet_owner_closure import source_inventory

parser = argparse.ArgumentParser()
parser.add_argument('--fix', action='append', required=True)
parser.add_argument('--result', action='append', required=True)
args = parser.parse_args()
root = Path.cwd().resolve()
path = root / policy.PREFIX / policy.TRANSITION_RECORD
assert not path.exists(), 'preserve sealed transition'
old = json.loads((root / policy.PREFIX / 'baseline-source.json').read_text())
current = source_inventory(root)
delta = {name: {'before': old['files'].get(name), 'after': current.get(name)}
         for name in sorted(set(old['files']) | set(current)) if old['files'].get(name) != current.get(name)}
assert set(delta) <= policy.FIX_PATHS | policy.ADMIN_PATHS, sorted(set(delta) - policy.FIX_PATHS - policy.ADMIN_PATHS)
results = []
for name in args.result:
    value = json.loads((root / name).read_text())
    assert value['exit'] == 0 and value['command'], 'focused result must be an actual successful command'
    results.append({'record': name, 'sha256': policy.original.sha(root / name), 'exit': 0})
value = {'schema': 'airs-v2/c31-source-transition/1',
         'time': datetime.datetime.now(datetime.timezone.utc).isoformat(),
         'baseline_head': policy.BASE_HEAD, 'baseline_digest': old['digest'],
         'current_head': subprocess.check_output(['git', 'rev-parse', 'HEAD'], text=True).strip(),
         'current_source_digest': policy.digest(current), 'source_delta': delta,
         'fix_commits': args.fix, 'focused_results': results,
         'meaning': 'Declared C31-only delta. Historical G7 is preserved; current application qualification and independent C31/C32/P09 reviews remain required.'}
path.write_text(json.dumps(value, indent=2) + '\n')
try:
    print(json.dumps(policy.verify_transition(root), indent=2))
except BaseException:
    # Retain failed candidate for diagnosis; do not silently amend a seal.
    raise
