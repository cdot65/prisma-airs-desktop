#!/usr/bin/env python3
"""Verify the recorded Kirk-to-Worf source checkpoint after a real remote clone."""
from pathlib import Path
import hashlib
import json
import subprocess

root = Path(__file__).resolve().parents[2]
history = root / 'docs/planning/worktree-preservation/2026-11-01'
cache = {}
errors = []


def check(name, expected):
    path = root / name
    if name not in cache:
        cache[name] = hashlib.sha256(path.read_bytes()).hexdigest() if path.is_file() else None
    if cache[name] != expected:
        errors.append(name)


source = json.loads((history / 'current-source.json').read_text())
for name, digest in source['files'].items():
    check(name, digest)
for row in json.loads((history / 'main-pending.json').read_text())['files']:
    check(row['path'], row['sha256'])
for row in json.loads((history / 'index.json').read_text())['files']:
    if 'sha256' in row:
        check(row['preserved_as'], row['sha256'])
artifact = json.loads((root / 'docs/planning/evidence/C29/live/L19/build-02/artifact-location.json').read_text())
check(artifact['artifact'], artifact['executable_sha256'])
for commit in source['required_commits']:
    if subprocess.run(['git', '-C', str(root), 'merge-base', '--is-ancestor', commit, 'HEAD'], capture_output=True).returncode:
        errors.append('missing required history ' + commit)
if errors:
    raise SystemExit('Migration checkpoint mismatch (fetch/pull main and Git LFS before continuing):\n' + '\n'.join(sorted(set(errors))))
print(json.dumps({'migration_checkpoint': 'pass', 'source_files': len(source['files']),
                  'distinct_files_verified': len(cache), 'historical_L19_executable': 'verified',
                  'head': subprocess.check_output(['git', '-C', str(root), 'rev-parse', 'HEAD'], text=True).strip()}, indent=2))
