"""Deliberate behavior mutations for C22's workspace MCP servers: the
catalog kept to the workspace's own HTTPS servers with order-independent
namespaces; the bootstrap's tokens and the receipt's echo; approval held
only for this turn's running call on a signed-in scope server and answered
as an elicitation; the desktop's OAuth carrying `resource`; tokens fitting
the engine; a scope retired by a new grant, server off or new revision,
needing its catalog, and refused for history from another scope; the
native decline for a lapsed sign-in; an MCP-only rotation restarting
without asking again; sign-out deleting the token item; and the
presentation and contract rules.

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
WS_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "workspace_mcp"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "harness"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_H = vitest("src/features/harness/harness-view.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (WS_T, UNIT_T, FE_H, FE_SNAPSHOT)

MCP = "crates/desktop-core/src/harness/mcp.rs"
AUTH = "crates/desktop-core/src/harness/mcp_auth.rs"
PROTOCOL = "crates/desktop-core/src/harness/protocol.rs"
SESSION = "crates/desktop-core/src/harness/session.rs"
HMCP = "crates/desktop-core/src/authority/harness_mcp.rs"
HTOOLS = "crates/desktop-core/src/authority/harness_tools.rs"
DOMAIN = "crates/desktop-core/src/authority/harness_domain.rs"
HDOMAIN = "crates/desktop-core/src/authority/harness_history.rs"
PANEL = "apps/desktop/src/features/harness/mcp-panel.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"

U_URL = "urls_are_https_canonical_and_never_carry_credentials"
U_NAMES = "namespaces_are_engine_names_and_independent_of_order"
U_CATALOG = "the_catalog_keeps_only_the_workspaces_usable_servers"
U_BOOT = "a_scope_is_bootstrapped_with_tokens_or_the_signed_out_placeholder"
U_RECEIPT = "mcp_servers_are_bootstrapped_in_scope_order_and_echoed_exactly"
U_ASK = "only_an_mcp_approval_naming_this_turns_running_call_is_asked"
U_ATTEMPT = "an_attempt_carries_pkce_state_resource_and_only_the_tool_scopes"
U_TOKENS = "tokens_must_be_bearer_and_fit_the_engine"
SIGN_IN = "the_catalog_is_read_per_workspace_and_signing_in_is_the_desktops_own_oauth"
RUNS = "an_approved_tool_call_runs_once_with_the_memory_only_bearer"
ROTATE = "mcp_token_rotation_restarts_and_restores_without_asking_for_changes_again"
RETIRE = "signing_out_again_or_off_or_a_new_revision_retires_the_conversation"
CATALOG = "without_its_catalog_a_scope_cannot_start_or_continue"
LAPSED = "an_approval_for_a_lapsed_sign_in_is_declined_natively"
SAVED = "a_saved_conversation_continues_only_with_its_mcp_scope"
FE_LIMIT = "says why a server cannot be used and keeps at most eight on"
SNAP = "refuses MCP views Rust never issues"
# P06 (D034 push to 9): C22 review F1-F4 and G1.
GRANTS = "crates/desktop-core/src/authority/harness_identity.rs"
EXPIRY = "an_approval_past_its_tokens_expiry_is_declined_and_inference_goes_on"
UNREADABLE = "a_catalog_that_cannot_be_read_blocks_remote_use_until_read_again"
SIGNED_OUT = "signing_out_during_a_sign_in_again_never_signs_the_server_back_in"
SETTLED = "a_written_token_survives_cleanup_until_it_is_settled"
CUT = "a_call_cut_short_is_recorded_as_interrupted_and_never_replayed"

PROBES = [
    ("another workspace's server is listed",
     [(MCP, "        if row[\"workspace_id\"].as_str() != Some(workspace) {", "        if row[\"workspace_id\"].as_str() == Some(\"\") {")], UNIT_T, U_CATALOG),
    ("a plain-HTTP server URL is kept",
     [(MCP, "    (url.scheme() == \"https\"", "    (url.scheme() != \"ftp\"")], UNIT_T, U_URL),
    ("colliding namespaces are not told apart",
     [(MCP, "        if group.len() == 1 && !RESERVED.contains(&base.as_str()) {", "        if group.len() >= 1 {")], UNIT_T, U_NAMES),
    ("a short token is given to the engine",
     [(MCP, "                    .filter(|t| valid_token(t))\n", "")], UNIT_T, U_BOOT),
    ("the receipt's MCP servers are not counted",
     [(PROTOCOL, "            && self.mcp_servers.len() == binding.mcp.len()", "            && true")], UNIT_T, U_RECEIPT),
    ("an elicitation for another server is asked",
     [(SESSION, "                m.id == item && Some(m.server.as_str()) == server && !m.asked && !m.done", "                m.id == item && !m.asked && !m.done")], UNIT_T, U_ASK),
    ("a signed-out server's call is asked",
     [(SESSION, "            held && self.mcp_items[i].reviewable && signed_in(&self.mcp_items[i].server)",
       "            held && self.mcp_items[i].reviewable || signed_in(&self.mcp_items[i].server)")], UNIT_T, U_ASK),
    ("an MCP approval is answered as a command",
     [(SESSION, "                        json!({\"action\": decision})", "                        json!({\"decision\": decision})")], UNIT_T, U_ASK),
    ("the sign-in leaves out its resource",
     [(AUTH, "            .append_pair(\"resource\", &provider.resource);", "            ;")], UNIT_T, U_ATTEMPT),
    ("an access token of any length is accepted",
     [(AUTH, "            .filter(|t| valid_token(t))\n", "")], UNIT_T, U_TOKENS),
    ("a new revision keeps the conversation",
     [(HMCP, "                .any(|c| c.id == s.server_id && c.revision == s.revision && c.url == s.url)",
       "                .any(|c| c.id == s.server_id && c.url == s.url)")], WS_T, RETIRE),
    ("signing in again keeps the conversation",
     [(HMCP, "                .is_some_and(|m| m.enabled && m.grant.as_ref().map(|g| &g.id) == s.grant.as_ref())",
       "                .is_some_and(|m| m.enabled)")], WS_T, RETIRE),
    ("a scope starts without its catalog",
     [(HMCP, "        let scope = self.mcp_scope_now(tenant, workspace).ok_or(Refusal::Busy)?;",
       "        let scope = self.mcp_scope_now(tenant, workspace).unwrap_or_default();")], WS_T, CATALOG),
    ("sign-out keeps the token item",
     [(HMCP, "        self.data.pending_cleanup.push(grant.record);\n", "        let _ = grant.record;\n")], WS_T, RETIRE),
    ("a lapsed sign-in's tool is run",
     [(HTOOLS, "            allow: allow && usable,", "            allow,")], WS_T, LAPSED),
    ("an MCP-only rotation asks for changes again",
     [(DOMAIN, "            if job.grant.is_some() && conversation.write {", "            if conversation.write {")], WS_T, ROTATE),
    ("a changed token does not restart the engine",
     [(DOMAIN, "                    (c.mcp_fingerprint.as_deref() != Some(print.as_str())).then(|| {",
       "                    (c.mcp_fingerprint.as_deref() == Some(\"\")).then(|| {")], WS_T, ROTATE),
    ("another scope's conversation is continuable",
     [(HDOMAIN, "        (self.mcp_scope_now(&h.tenant, &h.workspace).as_ref() != Some(&b.mcp))",
       "        (self.mcp_scope_now(&h.tenant, &h.workspace).is_none())")], WS_T, SAVED),
    ("more than eight servers can be turned on",
     [(PANEL, "disabled={busy || !s.supported || (!s.enabled && on >= MAX_ON)}", "disabled={busy || !s.supported}")], FE_H, FE_LIMIT),
    ("nine servers on pass the contract",
     [(CLIENT, "        servers.filter((s) => (s as { enabled: boolean }).enabled).length <= MAX_MCP_SERVERS", "        true")], FE_SNAPSHOT, SNAP),
    ("an MCP approval without a server passes the contract",
     [(CLIENT, "        (value.kind === 'mcpTool') === (value.server !== null && value.tool !== null) &&\n", "")], FE_SNAPSHOT, SNAP),
    # P06 follow-ups (docs/planning/reviews/C22/follow-ups.md).
    ("a token past its expiry runs a call",
     [(HMCP, "                .is_some_and(|l| l.usable_now())", "                .is_some_and(|l| l.expired().is_none())")], WS_T, EXPIRY),
    ("a failed catalog read still authorizes",
     [(HMCP, "                &c.tenant == tenant && c.workspace == workspace && !c.loading && c.failure.is_none()",
       "                &c.tenant == tenant && c.workspace == workspace")], WS_T, UNREADABLE),
    ("an unchecked scope sends",
     [(DOMAIN, "                    != super::harness_mcp::ScopeCheck::Current", "                    == super::harness_mcp::ScopeCheck::Changed")], WS_T, UNREADABLE),
    ("signing out leaves a sign-in under way",
     [(HMCP, "            if flow.tenant == tenant && flow.workspace == workspace.id && flow.server_id == server {",
       "            if flow.tenant == tenant && flow.workspace == workspace.id && flow.server_id == \"\" {")], WS_T, SIGNED_OUT),
    ("a write leaves flight before it is settled",
     [(GRANTS, "    let hold = lock(inner).map(|s| Arc::clone(&s.settle_hold));",
       "    if let Ok(mut s) = lock(inner) {\n        s.writes_in_flight.retain(|r| r != record);\n    }\n    let hold = lock(inner).map(|s| Arc::clone(&s.settle_hold));")], WS_T, SETTLED),
    ("an MCP call too large to show whole is asked",
     [(SESSION, "                let reviewable = whole.len() <= MAX_APPROVAL_COMMAND;", "                let reviewable = true;")], UNIT_T, U_ASK),
    ("a turn cut short is not saved",
     [(SESSION, "    if cut && let (Some(sink), Some(export)) = (sink, kept) {", "    if false && let (Some(sink), Some(export)) = (sink, kept) {")], WS_T, CUT),

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
