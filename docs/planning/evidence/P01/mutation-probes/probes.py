"""Deliberate behavior mutations for guards added in the P01 cumulative review.

Run in a disposable copy of the repository (never the reviewed checkout):
    python3 -I -B probes.py <copy-root> <results.json>
Each probe applies one mutation (one or more exact, asserted replacements)
that violates a guarded invariant, runs the guarding target, requires it to
fail on the named test, and restores the files. Unmutated baselines of every
target must pass.
"""
import json
import os
import subprocess
import sys

REPO = sys.argv[1]
HOME = os.environ["HOME"]
ENV = {"HOME": HOME, "PATH": f"{REPO}/.tools/bin:{HOME}/.cargo/bin:{HOME}/.nvm/versions/node/v24.21.0/bin:/usr/bin:/bin",
       "RUSTUP_TOOLCHAIN": "1.99.0", "CI": "1", "NO_COLOR": "1", "CARGO_TERM_COLOR": "never",
       "COREPACK_ENABLE_DOWNLOAD_PROMPT": "0", "TMPDIR": os.environ.get("TMPDIR", "/tmp")}
SCAN = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "sync_scan"]
CORE_LIB = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib"]
FE = ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts",
      "src/features/runtime/scan-submit.test.tsx", "src/contracts/snapshot-validation.test.ts"]

AUTH = "crates/desktop-core/src/authority.rs"
SCANNING = "crates/desktop-core/src/scanning.rs"
VIEW = "apps/desktop/src/features/runtime/runtime-view.tsx"
TEXT = "apps/desktop/src/features/runtime/scan-text.ts"
CLIENT = "packages/desktop-contracts/src/client.ts"
PROBES = [
    ("a refused tenant deletion drops its scan records",
     [(AUTH, "            state.data.tenants.retain(|t| t.profile.id != tenant);\n            if was_active {",
       "            state.data.tenants.retain(|t| t.profile.id != tenant);\n            state.scans.retain(|r| r.tenant != tenant);\n            if was_active {")],
     SCAN, "a_refused_tenant_deletion_keeps_its_scans_observable_and_cancellable"),
]


def run(cmd):
    try:
        p = subprocess.run(cmd, cwd=REPO, env=ENV, capture_output=True, text=True, timeout=600)
    except subprocess.TimeoutExpired:
        return "timeout", ""
    return p.returncode, p.stdout + p.stderr


def apply(edits):
    saved = {}
    for path, old, new in edits:
        full = os.path.join(REPO, path)
        text = saved.get(full) and open(full).read() or open(full).read()
        saved.setdefault(full, text)
        assert text.count(old) == 1, (path, old)
        with open(full, "w") as f:
            f.write(text.replace(old, new))
    return saved


def restore(saved):
    for full, text in saved.items():
        with open(full, "w") as f:
            f.write(text)


results = []
for name, edits, cmd, test in PROBES:
    saved = apply(edits)
    try:
        code, out = run(cmd)
    finally:
        restore(saved)
    results.append({"mutation": name, "files": sorted({e[0] for e in edits}), "command": cmd, "guard": test,
                    "exit_code": code, "guard_failed": code != 0 and test in out})
    print(name, code, results[-1]["guard_failed"], flush=True)
for cmd in (SCAN,):
    code, _ = run(cmd)
    results.append({"baseline": cmd, "exit_code": code, "passes": code == 0})
    print("baseline", cmd[-1], code, flush=True)
with open(sys.argv[2], "w") as f:
    json.dump(results, f, indent=2)
