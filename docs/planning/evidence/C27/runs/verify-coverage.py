#!/usr/bin/env python3 -I
"""Checks that every repo-relative file the compiler's dep-info (.d) files list
for the xtask, desktop-core and shell_controller build units is a member of
the retained input inventory. Usage: verify-coverage.py <repo> <inventory>
Prints a JSON report; exit 1 on any uncovered input."""
import glob, json, os, sys
repo, inventory = sys.argv[1], sys.argv[2]
members = {line.split(None, 1)[1].strip() for line in open(inventory) if line.strip()}
dep_files = sorted(f for pat in ("xtask-*.d", "desktop_core-*.d", "shell_controller-*.d")
                   for f in glob.glob(os.path.join(repo, "target/debug/deps", pat)))
deps, env_deps = set(), set()
for f in dep_files:
    for line in open(f):
        if line.startswith("#"):
            continue
        for tok in line.replace("\\\n", " ").split():
            tok = tok.rstrip(":")
            if tok.startswith("env-dep:"):
                env_deps.add(tok); continue
            if tok.endswith((".d", ".rlib", ".rmeta")) or not tok:
                continue
            path = tok if os.path.isabs(tok) else os.path.join(repo, tok)
            path = os.path.normpath(path)
            if path.startswith(os.path.normpath(repo) + os.sep) and "/target/" not in path:
                deps.add(os.path.relpath(path, repo))
uncovered = sorted(d for d in deps if d not in members)
report = {"dep_info_files": [os.path.relpath(f, repo) for f in dep_files], "repo_inputs_listed": sorted(deps),
          "env_deps": sorted(env_deps), "uncovered": uncovered, "covered": not uncovered}
print(json.dumps(report, indent=2))
sys.exit(1 if uncovered else 0)
