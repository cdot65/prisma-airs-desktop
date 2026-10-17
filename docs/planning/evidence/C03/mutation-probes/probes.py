"""Deliberate behavior mutations for C03's scan guards.

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
    ("an unrecognized action reads as allowed",
     [(SCANNING, "        (\"allow\", \"malicious\") => Verdict::Alert,\n        _ => Verdict::Unknown,",
       "        (\"allow\", \"malicious\") => Verdict::Alert,\n        _ => Verdict::Allow,")],
     SCAN, "unknown_actions_categories_and_incomplete_scans_are_never_allow"),
    ("a content-level error with false global flags reads as complete",
     [(SCANNING, "response.timeout || response.error || !response.errors.is_empty()", "response.timeout || response.error")],
     SCAN, "a_content_error_is_incomplete_whatever_the_global_flags_say"),
    # Not probed: the explicit `claim.cancel.is_cancelled()` check before
    # sending is defense in depth. Without it the already-cancelled request
    # token stops the SDK before dispatch (NotSent, nothing sent), so the
    # mutation is unobservable; cancelling_a_claimed_scan_before_it_is_sent_
    # sends_nothing asserts the behavior itself.
    ("Unknown text claims the request was sent",
     [(TEXT, "                : 'The outcome is unknown. The request may or may not have reached the service, and it may have been scanned. It is not resubmitted automatically.'",
       "                : 'The request was sent but the result is unknown. It is not resubmitted automatically.'")],
     FE, "reports a denied key and a lost response truthfully and never resubmits"),
    ("an incomplete scan reads as allowed",
     [(SCANNING, "        _ if incomplete => Verdict::Unknown,\n", "")],
     SCAN, "unknown_actions_categories_and_incomplete_scans_are_never_allow"),
    # Not probed: `with_retries(0)` is defense in depth. scm-sdk 0.1.0 never
    # replays a scan POST (not replay-safe; runtime retries are gateway-only,
    # engine.rs:1176/1226), so raising it is unobservable; the no-retry
    # behavior itself is asserted by the single-request checks.
    ("cancellation does not reach the in-flight request",
     [(SCANNING, "                if let Some(token) = &token {\n                    token.cancel();\n                }\n", "")],
     SCAN, "cancelling_a_running_scan_is_unknown_and_a_queued_one_is_not_sent"),
    ("a late reply for a retired context is shown",
     [(AUTH, "            if Some(&record.context) == current.as_ref() {", "            if true {")],
     SCAN, "a_tenant_switch_mid_request_discards_the_late_reply"),
    ("an earlier context's scan can be cancelled",
     [(AUTH, "            state.check_context(&record.context)?;\n", "")],
     SCAN, "a_tenant_switch_mid_request_discards_the_late_reply"),
    ("drafts admitted without validation",
     [(AUTH, "            let draft = draft.validate().ok_or(Refusal::InvalidInput)?;", "            let draft = draft;")],
     SCAN, "admission_requires_a_current_context_a_ready_key_and_a_valid_draft"),
    ("history kept without bound",
     [(AUTH, "            keep[i] = count <= HISTORY_PER_TENANT;", "            keep[i] = count > 0;")],
     SCAN, "history_keeps_the_newest_five_settled_scans_per_tenant"),
    ("the presentation sends a prompt over 2 MiB",
     [(TEXT, "    if (bytes(input.prompt) > MAX_TEXT_BYTES) return { problem: 'The prompt is larger than 2 MiB.', field: 'prompt' }\n", "")],
     FE, "sends working v1 inputs and the largest accepted request"),
    ("an unknown verdict is labelled allowed",
     [(TEXT, "    unknown: { label: 'Not determined', tone: 'warning' },", "    unknown: { label: 'Allowed', tone: 'warning' },")],
     FE, "never presents"),
    ("an unconfirmed scan delivery is resubmitted",
     [(VIEW, "            setMessage('The desktop did not confirm the scan. Check the history below before scanning again.')",
       "            void dispatch({ kind: 'scan', context, draft: built.draft })")],
     FE, "reports an unconfirmed scan delivery without resending it"),
    ("the validator accepts a result on an unfinished scan",
     [(CLIENT, "    if (value.result !== null && value.outcome !== 'completed') return false\n", "")],
     FE, "rejects a result on a scan that did not complete"),
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
for cmd in (SCAN, CORE_LIB, FE):
    code, _ = run(cmd)
    results.append({"baseline": cmd, "exit_code": code, "passes": code == 0})
    print("baseline", cmd[-1], code, flush=True)
with open(sys.argv[2], "w") as f:
    json.dump(results, f, indent=2)
