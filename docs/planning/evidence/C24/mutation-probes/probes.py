"""Deliberate behavior mutations for C24's closure: diagnostics that keep
only names and codes, re-check them on export, stay bounded and within
retention, are cleared and removed by reset, and record a panic only by a
repository path; xtask evaluating every registered budget for the cumulative
chunk; and the presentation and contract rules.

Run in a disposable copy of the repository (never the reviewed checkout):
    python3 -I -B probes.py <copy-root> <results.json> [name filter]
Each probe applies one mutation that violates a guarded invariant, runs the
guarding target, requires it to fail on the named test, and restores the
files. Unmodified baselines of every target must pass three runs, with their
output kept.
"""
import hashlib
import json
import os
import re
import subprocess
import sys

REPO = sys.argv[1]
HOME = os.environ["HOME"]
ENV = {"HOME": HOME, "PATH": f"{REPO}/.tools/bin:{HOME}/.cargo/bin:{HOME}/.nvm/versions/node/v24.21.0/bin:/usr/bin:/bin",
       "RUSTUP_TOOLCHAIN": "1.99.0", "CI": "1", "NO_COLOR": "1", "CARGO_TERM_COLOR": "never",
       "COREPACK_ENABLE_DOWNLOAD_PROMPT": "0", "TMPDIR": os.environ.get("TMPDIR", "/tmp")}
NQ_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "native_quality"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "diagnostics"]
XT_T = ["cargo", "test", "--locked", "-p", "xtask", "--features", "test-fixtures", "--lib", "budgets"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_D = vitest("src/features/diagnostics/diagnostics-panel.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (NQ_T, UNIT_T, XT_T, FE_D, FE_SNAPSHOT)

DIAG = "crates/desktop-core/src/diagnostics.rs"
AUTHORITY = "crates/desktop-core/src/authority.rs"
BUDGETS = "crates/xtask/src/budgets.rs"
PANEL = "apps/desktop/src/features/diagnostics/diagnostics-panel.tsx"
DOMAIN = "crates/desktop-core/src/authority/diagnostics_domain.rs"
CLIENT = "packages/desktop-contracts/src/client.ts"

KINDS = "diagnostics_keep_kinds_and_codes_and_never_a_secret_or_content"
# P07 follow-ups (review attempt-01).
KNOWN = "only_values_the_app_records_are_exported"
CLEAR_FAILED = "a_clearing_that_cannot_remove_the_files_says_so_and_keeps_counting_them"
FE_CLEAR = "announces a clearing, and says when one could not remove everything"
EXPORT = "diagnostics_are_exported_only_when_asked_and_redacted"
RESET = "clearing_and_reset_remove_the_diagnostics"
BOUNDED = "diagnostics_are_bounded_and_survive_a_relaunch"
PANIC = "a_panic_is_recorded_by_where_never_by_what"
U_NAMES = "only_names_and_codes_are_written"
U_BOUNDED = "the_log_is_bounded_kept_fourteen_days_and_purged"
X_CUMULATIVE = "the_cumulative_chunk_carries_every_workload_under_its_measuring_chunk"
FE_PANEL = "states what is kept, exports and clears only when asked, and announces the export"
SNAP = "rejects a view Rust never issues"

PROBES = [
    ("free text is written",
     [(DIAG, "        if !event.fields_ok() {\n            return;\n        }\n        let Ok(text)", "        let Ok(text)")], UNIT_T, U_NAMES),
    ("an edited line is exported",
     [(DIAG, "        .filter_map(|l| serde_json::from_str::<Line>(l).ok())\n        .filter(|l| l.event.fields_ok())\n",
       "        .filter_map(|l| serde_json::from_str::<Line>(l).ok())\n")], UNIT_T, U_NAMES),
    ("lines past retention are kept",
     [(DIAG, "                .filter(|l| at.saturating_sub(l.at) <= RETENTION_SECS)", "                .filter(|_| true)")], UNIT_T, U_BOUNDED),
    ("the log is never rotated",
     [(DIAG, "m.len() >= FILE_LIMIT)\n            && std::fs::rename", "m.len() >= u64::MAX)\n            && std::fs::rename")], NQ_T, BOUNDED),
    ("clearing keeps the lines",
     [(DIAG, "                match std::fs::remove_file(&path) {", "                match Ok::<(), std::io::Error>(()) {")], NQ_T, RESET),
    ("reset leaves the diagnostics",
     [(AUTHORITY, "                        if !worker.diagnostics.purge() {", "                        if worker.diagnostics.available() && false {")], NQ_T, RESET),
    ("an absolute panic path is recorded",
     [(AUTHORITY, "                file: if ours { file } else { \"external\" }.to_owned(),", "                file: file.to_owned(),")], NQ_T, PANIC),
    ("a refusal is not recorded",
     [(AUTHORITY, "            refusal: result.as_ref().err().map(refusal_code),", "            refusal: None,")], NQ_T, KINDS),
    ("the export is not saved",
     [(AUTHORITY, "            Effect::ExportDiagnostics { lease } => diagnostics_domain::export(inner, lease),",
       "            Effect::ExportDiagnostics { lease } => drop(lease),")], NQ_T, EXPORT),
    ("C24 evaluates no workloads",
     [(BUDGETS, "    subject == Subject::Chunk(CUMULATIVE)", "    subject == Subject::Chunk(0)")], XT_T, X_CUMULATIVE),
    ("C18's workload is unlisted",
     [(BUDGETS, "        registered_by: \"C18\",\n        registration: \"docs/planning/evidence/C18/workload-budget-registration.json\",\n        measured_by: \"C18\",",
       "        registered_by: \"C18\",\n        registration: \"docs/planning/evidence/C18/workload-budget-registration.json\",\n        measured_by: \"C18-unlisted\",")], XT_T, X_CUMULATIVE),
    ("the retention is not stated",
     [(PANEL, "Kept only on this Mac for {view.retentionDays} days and never sent anywhere", "Kept on this Mac")], FE_D, FE_PANEL),
    ("an uploaded export passes the contract",
     [(CLIENT, "optional((e) => oneOf(['saving', 'saved', 'cancelled', 'failed'], e), value.export)", "true")], FE_SNAPSHOT, SNAP),
    # ---- P07 follow-ups (review attempt-01) ----
    ("a token-shaped storage health is exported",
     [(DIAG, "            Event::Storage { health } => known_storage_health(health),", "            Event::Storage { health } => name_ok(health),")], NQ_T, KNOWN),
    ("a single-word request kind is exported",
     [(DIAG, "                known_intent_kind(kind) && refusal.as_deref().is_none_or(known_refusal)",
       "                name_ok(kind) && refusal.as_deref().is_none_or(known_refusal)")], NQ_T, KNOWN),
    ("a single-word refusal is exported",
     [(DIAG, "                known_intent_kind(kind) && refusal.as_deref().is_none_or(known_refusal)",
       "                known_intent_kind(kind) && refusal.as_deref().is_none_or(name_ok)")], NQ_T, KNOWN),
    ("any reset outcome is exported",
     [(DIAG, "            Event::Reset { outcome } => known_reset_outcome(outcome),", "            Event::Reset { outcome } => name_ok(outcome),")], NQ_T, KNOWN),
    ("any version is exported",
     [(DIAG, "            Event::Started { version, .. } => version_ok(version),", "            Event::Started { version, .. } => name_ok(version),")], NQ_T, KNOWN),
    ("a standard library panic path is recorded",
     [(DIAG, "            && (file.starts_with(\"crates/\") || file.starts_with(\"apps/\"))\n", "")], NQ_T, KNOWN),
    ("a failed clearing counts nothing",
     [(DIAG, "                        let kept = u32::try_from(read_lines(&path).len()).unwrap_or(u32::MAX);\n                        counter.store(kept, Ordering::Relaxed);",
       "                        counter.store(0, Ordering::Relaxed);")], NQ_T, CLEAR_FAILED),
    ("a failed clearing is recorded as cleared",
     [(DIAG, "        if removed {\n            self.record(Event::Cleared);\n        }", "        self.record(Event::Cleared);")], NQ_T, CLEAR_FAILED),
    ("a failed clearing reads cleared",
     [(DOMAIN, "            DiagnosticsClear::Failed\n", "            DiagnosticsClear::Cleared\n")], NQ_T, CLEAR_FAILED),
    ("a failed clearing is not said",
     [(PANEL, "{view.clear === 'failed' ? (", "{false ? (")], FE_D, FE_CLEAR),
    ("an unknown clearing passes the contract",
     [(CLIENT, "optional((c) => oneOf(['cleared', 'failed'], c), value.clear)", "true")], FE_SNAPSHOT, SNAP),
    # ---- P07 raise (native run of C24-N07) ----
    ("Export drops keyboard focus while its dialog is open",
     [(PANEL, "<Button type='button' tier='secondary' aria-disabled={busy} onClick={() => !busy && void run({ kind: 'exportDiagnostics' })}>",
       "<Button type='button' tier='secondary' disabled={busy} onClick={() => void run({ kind: 'exportDiagnostics' })}>")], FE_D, FE_PANEL),
    ("a held Export asks again",
     [(PANEL, "onClick={() => !busy && void run({ kind: 'exportDiagnostics' })}", "onClick={() => void run({ kind: 'exportDiagnostics' })}")], FE_D, FE_PANEL),
]


def run(cmd):
    try:
        p = subprocess.run(cmd, cwd=REPO, env=ENV, capture_output=True, text=True, timeout=900)
    except subprocess.TimeoutExpired:
        return "timeout", ""
    return p.returncode, p.stdout + p.stderr


def failed_on(test, out):
    """True only when the named test itself reports a failure (a Rust
    `test ... FAILED` line or a Vitest failure line naming it), so a build
    error or another test's failure never counts as the guard catching
    the mutation."""
    name = re.escape(test)
    return bool(re.search(rf"^test (\S+::)?{name} \.\.\. FAILED$", out, re.M)
                or re.search(rf"^.*(×|FAIL).*(> |× ){name}(\s+\d+ms)?\s*$", out, re.M))


def apply(edits):
    """Checks every anchor first, then writes; a bad anchor changes nothing."""
    texts = {}
    for path, old, _ in edits:
        full = os.path.join(REPO, path)
        texts.setdefault(full, open(full).read())
    pending = dict(texts)
    for path, old, new in edits:
        full = os.path.join(REPO, path)
        assert pending[full].count(old) == 1, (path, old)
        pending[full] = pending[full].replace(old, new)
    for full, text in pending.items():
        with open(full, "w") as f:
            f.write(text)
    return texts


def restore(saved):
    for full, text in saved.items():
        with open(full, "w") as f:
            f.write(text)
        assert digest(full) == INPUTS[os.path.relpath(full, REPO)], full


def digest(full):
    with open(full, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


# The files the probes mutate, as found in the copy: their hashes bind the
# run to the reviewed sources, and their modification times are renewed so
# no build made from an earlier mutation is reused (a copy made with rsync
# -a restores older times).
TARGETS = sorted({path for _, edits, _, _ in PROBES for path, _, _ in edits})
INPUTS = {path: digest(os.path.join(REPO, path)) for path in TARGETS}
for path in TARGETS:
    os.utime(os.path.join(REPO, path), None)
results = [{"inputs": INPUTS}]
# Unmodified baselines run first and three times each; their output is kept
# beside the results so any failure can be identified.
LOGS = os.path.join(os.path.dirname(os.path.abspath(sys.argv[2])), "baselines")
# An optional third argument runs only the probes whose name contains it.
ONLY = sys.argv[3] if len(sys.argv) > 3 else None
if not ONLY:
    os.makedirs(LOGS, exist_ok=True)
for index, cmd in enumerate(() if ONLY else BASELINES):
    codes = []
    for attempt in range(1, 4):
        code, out = run(cmd)
        codes.append(code)
        with open(os.path.join(LOGS, f"{index}-{cmd[-1].rsplit('/', 1)[-1]}-{attempt}.log"), "w") as f:
            f.write(out)
    results.append({"baseline": cmd, "exit_codes": codes, "passes": codes == [0, 0, 0]})
    print("baseline", cmd[-1], codes, flush=True)
for name, edits, cmd, test in PROBES:
    if ONLY and ONLY not in name:
        continue
    saved = apply(edits)
    try:
        code, out = run(cmd)
    finally:
        restore(saved)
    results.append({"mutation": name, "files": sorted({e[0] for e in edits}), "command": cmd, "guard": test,
                    "exit_code": code, "guard_failed": code != 0 and failed_on(test, out)})
    print(name, code, results[-1]["guard_failed"], flush=True)
with open(sys.argv[2], "w") as f:
    json.dump(results, f, indent=2)
