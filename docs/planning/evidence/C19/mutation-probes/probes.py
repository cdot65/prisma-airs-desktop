"""Deliberate behavior mutations for C19's native Harness: the engine
verified before it runs, the bootstrap receipt checked, frames bounded and
answers correlated, the stop closing input, killing the process group and
reaping, the runtime home removed, a conversation retired when its key,
workspace or context changes, on sleep and at quit, approvals declined,
answers bounded and the key redacted, the engine's own retries stopped,
one turn and one conversation at a time with bounded prompts; the host's
OS adapter (no inherited environment, its own process group, exit seen
without reaping, the group killed); and the presentation and contract
rules (Send only when ready, focus on the message box, https endpoints,
bounded entries, a failure always named).

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
HP_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "harness_process"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "harness"]
HOST_T = ["cargo", "test", "--locked", "-p", "desktop-host", "--features", "test-fixtures", "--test", "sidecar_supervision"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_H = vitest("src/features/harness/harness-view.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (HP_T, UNIT_T, HOST_T, FE_H, FE_SNAPSHOT)

ENGINE = "crates/desktop-core/src/harness/engine.rs"
SESSION = "crates/desktop-core/src/harness/session.rs"
DOMAIN = "crates/desktop-core/src/authority/harness_domain.rs"
AUTHORITY = "crates/desktop-core/src/authority.rs"
HOST = "apps/desktop/src-tauri/src/sidecar.rs"
VIEW = "apps/desktop/src/features/harness/harness-view.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"

STREAM = "a_conversation_streams_through_the_pinned_engine_with_only_the_workspace_key"
INTERRUPT = "stopping_an_answer_interrupts_only_that_turn"
RETIRE = "a_changed_key_workspace_or_context_retires_the_conversation_at_once"
SLEEP = "sleep_and_quit_end_the_conversation_and_reap_the_engine"
CORRUPT = "a_changed_or_missing_engine_is_never_started_and_no_key_is_read"
FAILED = "a_failed_answer_is_named_and_only_server_failures_are_retried_by_the_engine"
GIVEN = "the_engine_is_given_nothing_but_its_bootstrap"
FRAMES = "malformed_oversized_duplicate_or_foreign_frames_fail_the_conversation_and_reap_it"
KILLED = "an_engine_that_ignores_its_input_closing_is_killed_with_its_descendants"
APPROVAL = "approvals_are_declined_and_an_answer_past_its_bound_is_truncated"
ONE = "only_one_conversation_and_only_bounded_prompts_are_admitted"
BOUNDS = "completed_answers_and_token_counts_are_held_to_their_bounds"
U_PIN = "only_the_pinned_bytes_at_a_real_path_verify"
U_STUCK = "a_process_that_cannot_be_reaped_is_a_cleanup_failure_never_success"
U_HOME = "runtime_homes_are_private_ephemeral_and_only_ours_are_removed"
U_QUEUE = "a_full_queue_or_too_many_pending_requests_is_a_bound_never_a_wait"
H_ENV = "nothing_is_inherited_and_stderr_is_discarded"
H_EXIT = "closing_input_ends_a_reader_and_exit_is_seen_before_reaping"
H_GROUP = "killing_the_group_takes_descendants_with_it"
FE_STREAMS = "streams a conversation, sends with ⌘↩, stops an answer and ends it with focus following"
FE_OFFER = "offers a conversation only with the bundled engine, a chosen workspace and its stored key"
SNAP = "rejects conversations, entries and engines Rust never issues"
FE_NAMES = "names every failure and stop without the engine’s text and never shows the key"

PROBES = [
    ("an engine with other bytes runs",
     [(ENGINE, "    if hex::encode(hash.finalize()) != pin.binary_sha256 {", "    if hex::encode(hash.finalize()) == \"\" {")], HP_T, CORRUPT),
    # A link is refused twice over (not a regular file; not its canonical
    # path), so the link probe removes both guards; the canonical check alone
    # also refuses the real file reached through a linked folder.
    ("a link to the engine verifies",
     [(ENGINE, "    if !before.file_type().is_file() || before.len() > MAX_EXECUTABLE_BYTES {",
       "    if before.len() > MAX_EXECUTABLE_BYTES {"),
      (ENGINE, "    if path.canonicalize().map_err(unavailable)? != path {",
       "    if path.canonicalize().map_err(unavailable)?.as_os_str().is_empty() {")], UNIT_T, U_PIN),
    ("the engine is reached through a linked folder",
     [(ENGINE, "    if path.canonicalize().map_err(unavailable)? != path {",
       "    if path.canonicalize().map_err(unavailable)?.as_os_str().is_empty() {")], UNIT_T, U_PIN),
    ("the engine inherits the app's PATH",
     [(SESSION, '        ("PATH".into(), PATH.into()),\n',
       '        ("PATH".into(), std::env::var_os("PATH").unwrap_or_default()),\n')], HP_T, STREAM),
    ("the key is put in the engine's environment",
     [(SESSION, "        env: environment(&home, &shared.proxy),\n",
       "        env: { let mut e = environment(&home, &shared.proxy); e.push((\"GATEWAY_KEY\".into(), key.as_str().into())); e },\n")],
     HP_T, GIVEN),
    ("a receipt for another binding is accepted",
     [(SESSION, "                if !receipt.matches(&self.binding, &self.runtime_version) {",
       "                if receipt.v == 0 {")], HP_T, FRAMES),
    ("frames are not bounded",
     [(SESSION, "                if line.len() >= if first { FRAME_LIMIT } else { EVENT_LIMIT } {",
       "                if line.len() >= usize::MAX - usize::from(first) {")], HP_T, FRAMES),
    ("an answer to nothing pending is ignored",
     [(SESSION, "            .position(|(p, _)| p == n)\n            .ok_or(HarnessFailure::Protocol)?;\n        Ok(self.pending.remove(at).1)",
       "            .position(|(p, _)| p == n);\n        let Some(at) = at else {\n            return Ok(Method::TurnInterrupt);\n        };\n        Ok(self.pending.remove(at).1)")],
     HP_T, FRAMES),
    ("an engine ignoring its input is never killed",
     [(SESSION, "            child.kill_group();\n            how = Cleanup::Forced;", "            how = Cleanup::Forced;")], HP_T, KILLED),
    ("descendants outlive the engine",
     [(SESSION, "    child.kill_group();\n    match child.reap() {", "    match child.reap() {")], HP_T, KILLED),
    ("a stuck process is called stopped",
     [(SESSION, "        if started.elapsed() >= GRACE + FORCED {\n            return Cleanup::Failed;",
       "        if started.elapsed() >= GRACE + FORCED {\n            return Cleanup::Forced;")], UNIT_T, U_STUCK),
    ("the runtime home is left behind",
     [(SESSION, "    let removed = home.remove();", "    let removed = { let _ = &home; true };")], HP_T, STREAM),
    ("another run's home is removed",
     [(SESSION, "                .and_then(|n| n.to_str())\n                .is_some_and(is_run_dir);\n        ours &&",
       "                .and_then(|n| n.to_str())\n                .is_some_and(|_| true);\n        ours &&")], UNIT_T, U_HOME),
    ("an unbound conversation keeps running",
     [(DOMAIN, "            c.retired = true;\n            c.cancel.cancel();", "            c.retired = true;")], HP_T, RETIRE),
    ("a replaced key leaves the conversation bound",
     [(DOMAIN, "                ScanCapability::Ready { record, .. } if Some(record) == c.record.as_ref()",
       "                ScanCapability::Ready { .. }")],
     HP_T, RETIRE),
    ("sleep leaves the conversation running",
     [(AUTHORITY, "            && state.stop_harness(harness_domain::HarnessStop::Sleep)", "            && state.harness_running() && false")], HP_T, SLEEP),
    ("quitting does not stop the engine",
     [(AUTHORITY, "            && state.stop_harness(harness_domain::HarnessStop::Quit)", "            && state.harness_running() && false")], HP_T, SLEEP),
    ("requests pile up unanswered",
     [(SESSION, "        if self.pending.len() >= 8 {", "        if self.pending.len() >= 64 {")], UNIT_T, U_QUEUE),
    ("approvals are accepted",
     [(SESSION, 'protocol::reply(&id, json!({"decision":"decline"}))', 'protocol::reply(&id, json!({"decision":"accept"}))')], HP_T, APPROVAL),
    ("an answer is not bounded",
     [(SESSION, "        if over {\n            // Keep what fits", "        if over && false {\n            // Keep what fits")], HP_T, APPROVAL),
    ("the key is shown when an answer holds it",
     [(SESSION, "                if !secret.is_empty() && e.text.contains(secret) {", "                if secret.is_empty() && e.text.contains(secret) {")],
     HP_T, GIVEN),
    ("the engine's retries go on",
     [(SESSION, '            "error" if self.ours(params) && params["willRetry"] == true && !self.retry_refused => {',
       '            "error" if self.ours(params) && params["willRetry"] == true && self.retry_refused => {')], HP_T, FAILED),
    ("Stop answer sends nothing",
     [(SESSION, '            self.request(\n                Method::TurnInterrupt,\n                json!({"threadId":thread,"turnId":turn}),\n            )?;',
       '            let _ = (thread, turn);')], HP_T, INTERRUPT),
    ("a prompt is taken while answering",
     [(DOMAIN, "            .map(|p| p.phase != HarnessPhase::Ready)", "            .map(|p| p.phase == HarnessPhase::Failed)")], HP_T, INTERRUPT),
    ("an oversized prompt is admitted",
     [(DOMAIN, "        if prompt.trim().is_empty() || prompt.len() > protocol::MAX_PROMPT_BYTES {", "        if prompt.trim().is_empty() {")],
     HP_T, ONE),
    ("a second conversation is admitted",
     [(DOMAIN, "        if self.harness.as_ref().is_some_and(|c| c.ended.is_none()) {\n            return Err(Refusal::Busy);",
       "        if self.harness.as_ref().is_some_and(|c| c.ended.is_none() && c.retired) {\n            return Err(Refusal::Busy);")],
     HP_T, ONE),
    ("the host launcher inherits the environment",
     [(HOST, "            .env_clear()\n", "")], HOST_T, H_ENV),
    ("the host launcher shares the app's process group",
     [(HOST, "            .process_group(0)\n", "")], HOST_T, H_GROUP),
    ("observing exit reaps the engine",
     [(HOST, "                libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,", "                libc::WEXITED | libc::WNOHANG,")], HOST_T, H_EXIT),
    ("killing signals only the leader",
     [(HOST, "                libc::kill(-pid, libc::SIGKILL);", "                libc::kill(pid, libc::SIGKILL);")], HOST_T, H_GROUP),
    ("Send is offered while answering",
     [(VIEW, "disabled={phase !== 'ready' || busy || prompt.trim() === '' || tooLong}", "disabled={busy || prompt.trim() === '' || tooLong}")],
     FE_H, FE_STREAMS),
    ("focus stays put when the conversation is ready",
     [(VIEW, "        if (phase === 'ready' && was === 'starting') promptRef.current?.focus()\n", "")], FE_H, FE_STREAMS),
    ("a conversation is offered without this workspace's key",
     [(VIEW, "    const keyHere = key?.state === 'ready' && selected !== null && key.target?.workspace.id === selected.workspace.id",
       "    const keyHere = key?.state === 'ready' && selected !== null")], FE_H, FE_OFFER),
    ("a completion past the bound is shown complete",
     [(SESSION, "        let fits = text.len() <= allowed;", "        let fits = true;")], HP_T, BOUNDS),
    ("completed answers are not counted against the turn",
     [(SESSION, "        let turn_room = MAX_ANSWER_BYTES.saturating_sub(self.answer_bytes.saturating_sub(before));",
       "        let turn_room = MAX_ANSWER_BYTES;")], HP_T, BOUNDS),
    ("an unbounded completion item id names an entry",
     [(SESSION, "                    params[\"item\"][\"id\"].as_str().filter(|i| bounded_id(i)),",
       "                    params[\"item\"][\"id\"].as_str(),")], HP_T, FRAMES),
    ("an unsafe token count is published",
     [(SESSION, ".filter(|n| *n <= MAX_SAFE_COUNT)", ".filter(|_| true)")], HP_T, BOUNDS),
    ("a failed answer says it was not sent again",
     [(VIEW, "The engine may have sent the message to the Gateway up to five times before this app stopped it, and each may have been processed; nothing was undone.'",
       "It was not sent again.'")], FE_H, FE_NAMES),
    ("an http endpoint passes the contract",
     [(CLIENT, "        value.endpoint.startsWith('https://') &&\n        oneOf(HARNESS_PHASES, value.phase) &&\n",
       "        oneOf(HARNESS_PHASES, value.phase) &&\n")], FE_SNAPSHOT, SNAP),
    ("more entries than the bound pass the contract",
     [(CLIENT, "        optional((s) => oneOf(HARNESS_STOPS, s), value.stop) &&\n        Array.isArray(entries) &&\n        entries.length <= MAX_HARNESS_ENTRIES &&\n",
       "        optional((s) => oneOf(HARNESS_STOPS, s), value.stop) &&\n        Array.isArray(entries) &&\n")], FE_SNAPSHOT, SNAP),
    ("a failure without its name passes the contract",
     [(CLIENT, "        (value.phase !== 'failed' || value.failure !== null) &&\n", "")], FE_SNAPSHOT, SNAP),
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
