"""Deliberate behavior mutations for C21's Harness desktop tools, project
folder, approvals and proposals: a tool call run once and only for the turn
being answered, typed and bounded, never credential-bearing, scrubbed and
cut; approvals only with a folder, file changes only with changes allowed,
stale or repeated decisions refused; the folder and its access part of the
binding, unsafe folders refused, the read-only sandbox; proposals staged,
never replacing a review and closed once reviewed; the write
re-confirmation before a renewal; a restore needing its folder binding;
and the presentation and contract rules.

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
HT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "harness_tools"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "harness"]
HI_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "harness_identity_history"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_H = vitest("src/features/harness/harness-view.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (HT_T, HI_T, UNIT_T, FE_H, FE_SNAPSHOT)

TOOLS = "crates/desktop-core/src/harness/tools.rs"
PROTOCOL = "crates/desktop-core/src/harness/protocol.rs"
SESSION = "crates/desktop-core/src/harness/session.rs"
HTOOLS = "crates/desktop-core/src/authority/harness_tools.rs"
DOMAIN = "crates/desktop-core/src/authority/harness_domain.rs"
HDOMAIN = "crates/desktop-core/src/authority/harness_history.rs"
DECISIONS = "apps/desktop/src/features/harness/decisions-panel.tsx"
ADMIN = "apps/desktop/src/features/administration/administration-view.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"

READ = "a_typed_read_answers_from_the_desktop_without_any_secret"
FORGED = "forged_unknown_or_credential_bearing_calls_run_nothing"
PROPOSAL = "a_proposal_is_staged_never_applied_and_never_replaces_a_review"
COMMANDS = "commands_need_a_project_and_the_operators_approval"
SANDBOX = "the_sandbox_refuses_children_pipelines_and_other_paths"
PROJECT = "the_project_and_its_access_are_the_binding_and_unsafe_folders_are_refused"
RESTORE = "a_saved_conversation_needs_its_project_binding_and_replays_no_authority"
CONFIRM = "a_renewal_restarts_with_changes_only_once_the_operator_allows_them_again"
U_ONCE = "a_tool_call_runs_once_and_only_for_the_turn_being_answered"
U_PARSE = "calls_are_typed_bounded_and_never_credential_bearing"
U_RESULT = "results_are_scrubbed_redacted_and_bounded"
U_CATALOG = "the_catalog_fits_the_engine_and_forbids_unknown_fields"
U_ASK = "approvals_are_asked_only_with_a_folder_and_file_changes_only_with_changes_allowed"
FE_FOLDER = "chooses a folder, allows changes in it and removes it"
FE_DECIDE = "shows what the engine asks to do and sends the operator’s decision"
FE_REVIEW = "opens a proposal only in Administration’s review, and dismisses one"
FE_REFUSED = "says why a proposal could not be opened"
FE_CONFIRM = "asks again before a restarted engine may change the folder"
SNAP = "refuses tool, approval and proposal views Rust never issues"
# P06 (D034 push to 9): C21 review F1-F4 and its evidence gap.
U_LEASE = "a_stopped_turns_tool_calls_are_cancelled_refused_and_never_answered_late"
U_BOUND = "a_replaced_entry_keeps_the_transcript_within_its_bound"
STOPPED = "a_tool_call_under_way_when_its_turn_is_stopped_is_never_answered"
OFFLINE = "nothing_pending_survives_going_offline_and_nothing_is_resubmitted_on_reconnect"

PROBES = [
    ("a repeated tool call runs again",
     [(SESSION, "        if self.seen.contains(&call_id) {\n            return Err(HarnessFailure::Protocol);\n        }\n", "")], UNIT_T, U_ONCE),
    ("another turn's tool call runs",
     [(SESSION, "        let call = self.ours(params).then(|| {", "        let call = true.then(|| {")], UNIT_T, U_ONCE),
    ("credential-bearing arguments are accepted",
     [(TOOLS, "            || has_credentials(arguments)\n", "")], UNIT_T, U_PARSE),
    ("oversized arguments are accepted",
     [(TOOLS, "            || serde_json::to_vec(arguments).map_or(true, |b| b.len() > MAX_ARGUMENTS_BYTES)\n", "")], UNIT_T, U_PARSE),
    ("the DLP catalog is offered",
     [(TOOLS, "    !matches!(resource, ManagementResource::DlpCatalog)", "    let _ = resource;\n    true")], HT_T, FORGED),
    ("credential fields reach the model",
     [(TOOLS, "            map.retain(|k, _| !sensitive(k));\n", "")], UNIT_T, U_RESULT),
    ("results are not cut",
     [(TOOLS, "    if text.len() > MAX_RESULT_BYTES {", "    if text.len() > usize::MAX - 1 {")], UNIT_T, U_RESULT),
    ("the catalog allows unknown fields",
     [(TOOLS, "            \"additionalProperties\":false}})", "            \"additionalProperties\":true}})")], UNIT_T, U_CATALOG),
    ("a command is asked without a folder",
     [(SESSION, "            && self.binding.project.is_some()\n", "")], UNIT_T, U_ASK),
    ("a file change is asked while changes are off",
     [(SESSION, "            && (kind == ApprovalKind::Command || self.binding.write)\n", "")], UNIT_T, U_ASK),
    ("a used or stale approval is accepted",
     [(HTOOLS, "            return Err(Refusal::StaleContext);\n        };\n        // An MCP tool", "            return Ok(());\n        };\n        // An MCP tool")], HT_T, COMMANDS),
    ("system folders are a project",
     [(HTOOLS, "    if REFUSED_ROOTS.iter().any(|r| path.starts_with(r)) {", "    if REFUSED_ROOTS.is_empty() {")], HT_T, PROJECT),
    ("the home folder is a project",
     [(HTOOLS, "        && (path == home || REFUSED_HOME.iter().any(|d| path.starts_with(home.join(d))))",
       "        && REFUSED_HOME.iter().any(|d| path.starts_with(home.join(d)))")], HT_T, PROJECT),
    ("the app's data is a project",
     [(HTOOLS, "        && (path.starts_with(data) || data.starts_with(path))", "        && false")], HT_T, PROJECT),
    ("changing access keeps the conversation",
     [(DOMAIN, "            && project.is_some_and(|p| p.write) == c.write;", ";")], HT_T, PROJECT),
    ("a read-only thread may write",
     [(PROTOCOL, "        \"read-only\"\n", "        \"workspace-write\"\n")], HT_T, PROJECT),
    ("a proposal replaces the open review",
     [(HTOOLS, "        if self.change_review_open() {\n            return Err(Refusal::Busy);\n        }\n", "")], HT_T, PROPOSAL),
    ("a reviewed proposal stays in review",
     [(DOMAIN, "                                v.state = super::harness_tools::ProposalState::Closed;\n", "")], HT_T, PROPOSAL),
    ("a renewal restarts with changes unasked",
     [(DOMAIN, "            if job.grant.is_some() && conversation.write {", "            if job.grant.is_some() && conversation.write && false {")], HI_T, CONFIRM),
    ("Not now restarts anyway",
     [(HTOOLS, "        if allow {", "        if allow || !allow {")], HI_T, CONFIRM),
    ("another folder's conversation is continuable",
     [(HDOMAIN, "            return Some(NotContinuable::OtherProject);", "            let _ = NotContinuable::OtherProject;")], HT_T, RESTORE),
    ("Allow sends a decline",
     [(DECISIONS, "                                    <Button type='button' tier='primary' size='sm' disabled={busy} onClick={() => void decide(a, true)}>",
       "                                    <Button type='button' tier='primary' size='sm' disabled={busy} onClick={() => void decide(a, false)}>")], FE_H, FE_DECIDE),
    ("a refused review still leaves the Harness",
     [(DECISIONS, "        if (opened) onNavigate('administration')", "        onNavigate('administration')")], FE_H, FE_REFUSED),
    ("Administration shows a reformatted draft",
     [(ADMIN, "            body: d.body,", "            body: pretty(d.body),")], FE_H, FE_REVIEW),
    ("Not now allows changes",
     [(DECISIONS, "onClick={() => void send({ kind: 'confirmHarnessWrite', conversation: id, allow: false }, 'That was not done.')}",
       "onClick={() => void send({ kind: 'confirmHarnessWrite', conversation: id, allow: true }, 'That was not done.')}")], FE_H, FE_CONFIRM),
    ("changes without a folder pass the contract",
     [(CLIENT, "        (!value.write || value.project !== null) &&\n", "")], FE_SNAPSHOT, SNAP),
    ("a Gateway proposal passes the contract",
     [(CLIENT, "const PROPOSAL_KINDS: readonly string[] = CHANGE_KINDS.filter((k) => !k.startsWith('gateway'))",
       "const PROPOSAL_KINDS: readonly string[] = CHANGE_KINDS")], FE_SNAPSHOT, SNAP),
    ("a file-change approval while read-only passes the contract",
     [(CLIENT, "                return kind === 'mcpTool' || (value.project !== null && (value.write || kind === 'command'))",
       "                return kind === 'mcpTool' || value.project !== null || kind === 'command'")], FE_SNAPSHOT, SNAP),
    # P06 follow-ups (docs/planning/reviews/C21/follow-ups.md).
    ("a stale lease's tool result is sent",
     [(SESSION, ".position(|(r, _, l)| *r == request && *l == lease)", ".position(|(r, _, _)| *r == request)")], UNIT_T, U_LEASE),
    ("a tool call is admitted while its turn is stopped",
     [(SESSION, "        let call = call.filter(|_| !self.interrupt_wanted);\n", "")], UNIT_T, U_LEASE),
    ("a stopped turn's tool calls are not cancelled",
     [(SESSION, "        self.latest = None;\n        self.turn_cancel.cancel();\n", "        self.latest = None;\n")], UNIT_T, U_LEASE),
    ("JSON inside a string is not scrubbed",
     [(TOOLS, "    structured(&mut value, 0);\n    scrub(&mut value);", "    scrub(&mut value);")], UNIT_T, U_RESULT),
    ("a command too long to show whole is asked",
     [(SESSION, ".is_some_and(|c| !c.is_empty() && c.len() <= MAX_APPROVAL_COMMAND),", ".is_some_and(|c| !c.is_empty()),")], UNIT_T, U_ASK),
    ("a file change naming no files is asked",
     [(SESSION, "            ApprovalKind::FileChange => !files.is_empty(),", "            ApprovalKind::FileChange => true,")], UNIT_T, U_ASK),
    ("a replaced entry bypasses the transcript bound",
     [(SESSION, "            while transcript_bytes(p) > MAX_TRANSCRIPT_BYTES && p.entries.len() > 1 {", "            while false && p.entries.len() > 1 {")], UNIT_T, U_BOUND),

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
