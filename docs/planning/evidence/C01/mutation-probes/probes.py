"""Deliberate behavior mutations for C01's security-critical tests.

Run in a disposable copy of the repository (never the reviewed checkout):
    python3 probes.py <copy-root> <results.json>
Each probe applies one mutation that violates a guarded invariant, runs the
guarding target, requires it to fail on the named test, and restores the file.
Two probes are defense-in-depth controls that must still pass.
"""
import json
import os
import subprocess
import sys

REPO = sys.argv[1]
HOME = os.environ["HOME"]
ENV = {"HOME": HOME, "PATH": f"{REPO}/.tools/bin:{HOME}/.cargo/bin:{HOME}/.nvm/versions/node/v24.21.0/bin:/usr/bin:/bin",
       "RUSTUP_TOOLCHAIN": "1.99.0", "CI": "1", "NO_COLOR": "1", "CARGO_TERM_COLOR": "never", "COREPACK_ENABLE_DOWNLOAD_PROMPT": "0"}
XT = ["cargo", "test", "--locked", "-p", "xtask", "--features", "test-fixtures", "--test", "shell_controller"]
HOST = ["cargo", "test", "--locked", "-p", "desktop-host", "--features", "test-fixtures", "--test", "ipc_admission"]
FE = ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", "src/contracts/invoke.test.ts"]
FE_SHELL = ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", "src/shell/desktop-shell.test.tsx"]
SHARED5 = ["pnpm", "--dir", ".work/academy", "-r", "--fail-if-no-match"] + sum(
    [["--filter", f"@ai-security-academy/{n}"] for n in ["cli", "presentation", "rag-svc", "shared", "ui", "video"]], []) + ["run", "typecheck"]

GATE = "crates/xtask/src/gate.rs"
PROBES = [
    ("gate threshold lowered to 8", [(GATE, "pub const MINIMUM_SCORE: u8 = 9;", "pub const MINIMUM_SCORE: u8 = 8;")], XT, "score_eight_in_any_dimension_is_rejected"),
    ("verdict hash not checked", [(GATE, "    if sha256_hex(verdict_bytes) != receipt.verdict_sha256 {", "    if false && sha256_hex(verdict_bytes) != receipt.verdict_sha256 {")], XT, "edited_verdict_after_receipt_is_rejected"),
    ("expired attempts accepted", [(GATE, "    if inputs.attempt_expired {", "    if false && inputs.attempt_expired {")], XT, "expired_attempts_cannot_advance"),
    ("run command coverage not checked", [(GATE, "        if record.commands.len() != plan.len() {", "        if false && record.commands.len() != plan.len() {")], XT, "run_records_must_cover_the_exact_approved_plan"),
    ("final reviewer output not compared", [(GATE, ".any(|message| json_objects(message).contains(&expected));", ".any(|message| !message.is_empty() || json_objects(message).contains(&expected));")], XT, "failed_or_incomplete_reviewer_turns_cannot_complete"),
    ("native status ignores phase members", [("crates/xtask/src/native.rs", "match manifest.native_members(subject) {", "match manifest.native_members(Subject::Chunk(1)).map(|_| vec![subject.to_string()]) {")], XT, "native_status_and_completion_resolve_phase_members_alike"),
    ("bundle artifact not checked", [(GATE, "if bundle.is_none() || bundle != native.prepared_bundle_sha256 {", "if false && (bundle.is_none() || bundle != native.prepared_bundle_sha256) {")], XT, "prepared_bundle_bytes_path_and_identifier_are_bound"),
    ("web scratch drift ignored", [("crates/xtask/src/fingerprint.rs", "        && academy_consumer_hash(&input_tree)? != academy_consumer_hash(&scratch_tree)?", "        && false")], XT, "edits_to_the_web_consumer_scratch_tree_are_detected"),
    ("changed duplicate accepted", [("crates/desktop-core/src/delivery.rs", "Some(r) if &r.digest == digest => Admit::Duplicate(r.result.clone()),", "Some(r) => Admit::Duplicate(r.result.clone()),")], HOST, "delivery_sequence_rules_hold_through_ipc"),
    ("JSON body admitted without size ceiling", [("apps/desktop/src-tauri/src/lib.rs", "    let InvokeBody::Raw(bytes) = request.body() else {\n        return Err(Refusal::InvalidInput);\n    };\n    let request = DispatchRequest::parse(bytes)", "    let owned = match request.body() { InvokeBody::Raw(b) => b.clone(), InvokeBody::Json(v) => serde_json::to_vec(v).unwrap_or_default() };\n    let bytes = &owned;\n    let request = DispatchRequest::parse(bytes)")], HOST, "json_bodies_and_oversized_payloads_are_refused"),
    ("observation cap evicts instead of refusing", [("apps/desktop/src-tauri/src/lib.rs", "        if held >= MAX_OBSERVATIONS_PER_WEBVIEW {", "        if held >= MAX_OBSERVATIONS_PER_WEBVIEW * 100 {")], HOST, "observations_per_webview_are_bounded_without_silent_eviction"),
    ("focused control not scrolled into view", [("apps/desktop/src/shell/desktop-shell.tsx", "        document.addEventListener('focusin', reveal)\n", "")], FE_SHELL, "scrolls a control into view when arrow keys move focus within a radio group"),
    ("theme applied after the shell is painted", [("apps/desktop/src/app.tsx", "    useLayoutEffect(() => (appearance ? followAppearance(appearance) : undefined), [appearance])", "    useEffect(() => (appearance ? followAppearance(appearance) : undefined), [appearance])")], FE_SHELL, "applies the native theme in the same commit that first shows the shell"),
    ("sequence reused after lost result", [("packages/desktop-contracts/src/client.ts", "        this.#session = null\n        throw new DeliveryOutcomeUnknown", "        throw new DeliveryOutcomeUnknown")], FE, "never reuses a sequence"),
    ("non-main window admitted (capability and host check both relaxed)", [
        ("apps/desktop/src-tauri/capabilities/main.json", '"windows": ["main"],\n  "webviews": ["main"],', '"windows": ["main", "intruder"],\n  "webviews": ["main", "intruder"],'),
        ("apps/desktop/src-tauri/src/lib.rs", "    if webview.label() != MAIN_WINDOW || webview.window().label() != MAIN_WINDOW {", "    if false && (webview.label() != MAIN_WINDOW || webview.window().label() != MAIN_WINDOW) {")],
     HOST, "other_windows_and_remote_origins_cannot_reach_commands"),
]
CONTROLS = [
    ("capability relaxed only (host window check still rejects)", [("apps/desktop/src-tauri/capabilities/main.json", '"windows": ["main"],\n  "webviews": ["main"],', '"windows": ["main", "intruder"],\n  "webviews": ["main", "intruder"],')], HOST),
]


def run(cmd):
    p = subprocess.run(cmd, cwd=REPO, env=ENV, capture_output=True, text=True)
    return p.returncode, p.stdout + p.stderr


def apply(edits):
    saved = []
    for path, old, new in edits:
        full = os.path.join(REPO, path)
        text = open(full).read()
        assert text.count(old) == 1, (path, old)
        saved.append((full, text))
        open(full, "w").write(text.replace(old, new))
    return saved


def restore(saved):
    for full, text in saved:
        open(full, "w").write(text)


results = []
for name, edits, cmd, test in PROBES:
    saved = apply(edits)
    try:
        code, out = run(cmd)
    finally:
        restore(saved)
    results.append({"mutation": name, "files": [e[0] for e in edits], "command": cmd, "guard": test, "exit_code": code,
                    "guard_failed": code != 0 and test in out})
    print(name, code, results[-1]["guard_failed"], flush=True)
for name, edits, cmd in CONTROLS:
    saved = apply(edits)
    try:
        code, _ = run(cmd)
    finally:
        restore(saved)
    results.append({"control": name, "command": cmd, "exit_code": code, "still_passes": code == 0})
    print(name, code, flush=True)

# Type error injected into a package-only file must fail the shared package typecheck.
probe = os.path.join(REPO, ".work/academy/packages/presentation/src/__probe_type_error.ts")
open(probe, "w").write("export const probe: number = 'not a number'\n")
try:
    code, out = run(SHARED5)
finally:
    os.remove(probe)
results.append({"mutation": "type error in a presentation-only file", "command": SHARED5, "exit_code": code,
                "guard_failed": code != 0 and "packages/presentation" in out})
print("shared typecheck type error", code, flush=True)
code, out = run(SHARED5)
results.append({"control": "shared package typecheck without the injected error", "exit_code": code, "still_passes": code == 0,
                "packages_reported": sorted({l.split()[0] for l in out.splitlines() if l.startswith("packages/") and "typecheck: Done" in l})})
code, _ = run(XT)
results.append({"baseline": "unmutated shell_controller", "exit_code": code})
json.dump(results, open(sys.argv[2], "w"), indent=2)
