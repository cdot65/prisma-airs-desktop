"""Deliberate behavior mutations for C20's Harness company sign-in, renewal
and protected history: the provider's metadata kept to its issuer and S256,
both tokens' signature, issuer, audience, client, nonce, times and
workspace checked, opaque tokens refused, the loopback callback taking only
its own state, host and issuer; a renewal before a message inside the
margin, restoring the conversation, never sending after a refused one,
another account refused and signing out ending the grant; history sealed
and bound to its scope, a full workspace refusing rather than evicting,
exports without the replay, the running conversation kept, continuation
only with the same binding, a tenant's history removed with it; and the
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
HP_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "harness_process"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "harness"]
HI_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "harness_identity_history"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_H = vitest("src/features/harness/harness-view.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (HI_T, UNIT_T, FE_H, FE_SNAPSHOT)

IDENTITY = "crates/desktop-core/src/harness/identity.rs"
CALLBACK = "crates/desktop-core/src/harness/callback.rs"
HISTORY = "crates/desktop-core/src/harness/history.rs"
SESSION = "crates/desktop-core/src/harness/session.rs"
DOMAIN = "crates/desktop-core/src/authority/harness_domain.rs"
GRANTS = "crates/desktop-core/src/authority/harness_identity.rs"
HDOMAIN = "crates/desktop-core/src/authority/harness_history.rs"
AUTHORITY = "crates/desktop-core/src/authority.rs"
VIEW = "apps/desktop/src/features/harness/harness-view.tsx"
PANEL = "apps/desktop/src/features/harness/history-panel.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"

SIGNED = "company_sign_in_verifies_both_tokens_and_keeps_only_the_refresh_token"
REFUSED = "tokens_or_providers_that_do_not_verify_are_refused_and_nothing_is_kept"
RENEW = "a_renewal_restarts_the_engine_and_restores_the_conversation"
GIVEN_BACK = "a_refused_renewal_gives_the_message_back_and_a_new_sign_in_continues"
ENDED = "another_account_or_signing_out_ends_the_grant_and_its_conversation"
SEALED = "history_is_sealed_after_every_turn_and_read_exported_and_deleted"
CONTINUES = "a_saved_conversation_continues_only_with_its_binding"
NO_KEY = "without_its_key_history_is_never_written_in_plaintext_and_reads_unreadable"
FULL = "a_full_workspace_refuses_new_history_and_never_evicts"
TENANT = "removing_the_tenant_removes_its_history_its_key_and_its_sign_in"
U_CALLBACK = "only_this_attempts_callback_is_taken"
U_SCOPE = "a_record_is_sealed_and_opens_only_in_its_own_scope_and_name"
U_FULL = "a_full_scope_refuses_new_records_and_ids_are_uuids"
U_EXPORT = "exports_carry_the_transcript_only"
U_ATTEMPT = "an_attempt_carries_pkce_state_nonce_and_the_loopback"
U_MARGIN = "a_token_is_renewed_inside_its_margin"
FE_SIGN_IN = "sets up company sign-in, signs in through the browser, chooses it and signs out"
FE_GIVEN = "gives a message back when its renewal fails and shows the conversation as saved or not"
FE_LIST = "lists, opens, continues, exports and deletes saved conversations"
FE_WHY = "says why a saved conversation cannot be continued, names unreadable ones and a full workspace"
SNAP = "rejects sign-ins, saved conversations and exports Rust never issues"

PROBES = [
    ("discovery may name another issuer",
     [(IDENTITY, "        if status != 200 || string(body.get(\"issuer\"), 2048) != Some(config.issuer.as_str()) {",
       "        if status != 200 {")], HI_T, REFUSED),
    ("an endpoint off the issuer's origin is used",
     [(IDENTITY, "                .filter(|u| u.origin().ascii_serialization() == origin)\n", "")], HI_T, REFUSED),
    ("a provider without S256 is used",
     [(IDENTITY, "        if !listed(\"code_challenge_methods_supported\", \"S256\")\n            || !listed",
       "        if !listed")], HI_T, REFUSED),
    ("the challenge is not the verifier's digest",
     [(IDENTITY, "        .encode(Sha256::digest(verifier.as_bytes()));", "        .encode(verifier.as_bytes());")], UNIT_T, U_ATTEMPT),
    ("a forged signature is accepted",
     [(IDENTITY, "    jsonwebtoken::decode::<Map<String, Value>>(token, &key, &validation)\n        .map(|data| data.claims)",
       "    let _ = (&key, &validation);\n    jsonwebtoken::dangerous::insecure_decode::<Map<String, Value>>(token)\n        .map(|data| data.claims)")], HI_T, REFUSED),
    ("the access token's issuer is not checked",
     [(IDENTITY, "        if string(claims.get(\"iss\"), 2048) != Some(config.issuer.as_str())\n            || !audience_has(&claims, &config.audience).0",
       "        if !audience_has(&claims, &config.audience).0")], HI_T, REFUSED),
    ("the access token's audience is not checked",
     [(IDENTITY, "            || !audience_has(&claims, &config.audience).0", "            || false")], HI_T, REFUSED),
    ("another client's access token is accepted",
     [(IDENTITY, "                && value.as_str() != Some(config.client_id.as_str())", "                && false")], HI_T, REFUSED),
    ("an expired token is accepted",
     [(IDENTITY, "    if exp + SKEW <= now || iat > now + SKEW || iat >= exp {", "    if iat > now + SKEW || iat >= exp {")], HI_T, REFUSED),
    ("a token not yet valid is accepted",
     [(IDENTITY, "        && nbf.as_u64()? > now + SKEW", "        && nbf.as_u64()? > u64::MAX - 1")], HI_T, REFUSED),
    ("the ID token's nonce is not checked",
     [(IDENTITY, "        if !nonce_ok || timely(&claims, now).is_none() {", "        if timely(&claims, now).is_none() {")], HI_T, REFUSED),
    ("an ID token for another client is accepted",
     [(IDENTITY, "        if !has\n            || (count > 1", "        if (count > 1")], HI_T, REFUSED),
    ("another workspace's token is accepted",
     [(IDENTITY, "            && named.as_str() != Some(workspace_slug)", "            && false")], HI_T, REFUSED),
    ("an opaque token is only invalid",
     [(IDENTITY, "            return Err(SignInFailure::OpaqueToken);", "            return Err(SignInFailure::InvalidToken);")], HI_T, REFUSED),
    ("another account renews into the grant",
     [(IDENTITY, "        if expected.is_some_and(|e| e != subject) {", "        if expected.is_some_and(|e| e.is_empty()) {")], HI_T, ENDED),
    ("another state completes the attempt",
     [(CALLBACK, "    if !given_state.is_some_and(|s| constant_eq(s.as_bytes(), state.as_bytes())) {",
       "    if given_state.is_none() {")], UNIT_T, U_CALLBACK),
    ("another Host is accepted",
     [(CALLBACK, "    if hosts != [host] {", "    if hosts.is_empty() {")], UNIT_T, U_CALLBACK),
    ("another issuer's callback is accepted",
     [(CALLBACK, "    if iss.map_or(iss_required, |i| i != issuer) {", "    if iss.is_none() && iss_required {")], UNIT_T, U_CALLBACK),
    ("a token is never renewed before a message",
     [(DOMAIN, "            (Some(grant), Some(at)) if now() >= at => Some(grant),", "            (Some(grant), Some(at)) if at == 0 => Some(grant),")], HI_T, RENEW),
    ("a renewal margin of nothing",
     [(IDENTITY, "        self.expires.saturating_sub((lifetime / 2).min(120))", "        self.expires")], UNIT_T, U_MARGIN),
    ("a renewed engine starts a new thread",
     [(SESSION, "                    restore: latest,\n", "                    restore: None,\n")], HI_T, RENEW),
    ("a refused renewal still sends with the old token",
     [(DOMAIN, "            Err(failure) => {\n                c.entries -= 1;",
       "            Err(failure) if c.commands.try_send(Command::Send { id: job.id.clone(), prompt: job.prompt.clone() }).is_err() => {\n                c.entries -= 1;"),
      (DOMAIN, "                if let Ok(mut p) = c.progress.lock() {\n                    p.phase = HarnessPhase::Ready;\n                }\n            }\n        }\n    }",
       "                if let Ok(mut p) = c.progress.lock() {\n                    p.phase = HarnessPhase::Ready;\n                }\n            }\n            Err(_) => {}\n        }\n    }")], HI_T, GIVEN_BACK),
    ("signing out keeps the conversation",
     [(DOMAIN, "                .is_some_and(|g| g.id == grant.id && s.target.endpoint == c.endpoint),",
       "                .map_or(true, |g| g.id == grant.id && s.target.endpoint == c.endpoint),")], HI_T, ENDED),
    ("signing out keeps the refresh token",
     [(GRANTS, "        self.data.pending_cleanup.push(grant.record);\n        self.save_with(&before)?;",
       "        let _ = grant.record;\n        self.save_with(&before)?;")], HI_T, ENDED),
    ("history is written in plaintext",
     [(HISTORY, "    let sealed = crate::sessions::seal(&key, &scope.aad(&record.id), &plain)\n        .ok_or(HistoryFailure::Unavailable)?;",
       "    let _ = &key;\n    let sealed = plain.to_vec();")], HI_T, SEALED),
    ("a record opens in another scope",
     [(HISTORY, "        format!(\"{SCHEMA}:{}:{id}\", self.digest).into_bytes()", "        format!(\"{SCHEMA}:{id}\").into_bytes()")], UNIT_T, U_SCOPE),
    ("a full workspace saves another",
     [(HISTORY, "    if !ids.contains(&record.id) && ids.len() >= MAX_CONVERSATIONS {", "    if !ids.contains(&record.id) && ids.len() > MAX_CONVERSATIONS {")], HI_T, FULL),
    ("an export carries the replay",
     [(HISTORY, "            \"entries\": record.entries,\n        }))", "            \"entries\": record.entries,\n            \"replay\": record.replay,\n        }))")], HI_T, SEALED),
    ("the running conversation can be deleted",
     [(HDOMAIN, "        if self.harness_history_live() == Some(id) {\n            return Err(Refusal::Busy);\n        }\n", "")], HI_T, SEALED),
    ("another binding's conversation is continuable",
     [(HDOMAIN, "        (!same).then_some(NotContinuable::OtherCredential)", "        let _ = same;\n        None")], HI_T, CONTINUES),
    ("a locked Keychain is not reported",
     [(SESSION, "            Err(f) => p.save_failure = Some(f),", "            Err(_) => {}")], HI_T, NO_KEY),
    ("a removed tenant's history stays on disk",
     [(AUTHORITY, "            if let Some(dir) = history_dir {\n                effects.push(Effect::ForgetHarnessHistory(dir));\n            }",
       "            let _ = history_dir;")], HI_T, TENANT),
    ("a message given back is dropped",
     [(VIEW, "            setPrompt((current) => (current === '' ? unsent : current))\n", "")], FE_H, FE_GIVEN),
    ("Continue is offered for a conversation that cannot continue",
     [(PANEL, "                        {opened.notContinuable === null ? (", "                        {opened.notContinuable !== undefined ? (")], FE_H, FE_WHY),
    ("Delete does not ask first",
     [(PANEL, "                                onClick={() => setConfirming(opened.id)}",
       "                                onClick={() => void send({ kind: 'deleteHarnessHistory', context, workspace, id: opened.id })}")], FE_H, FE_LIST),
    ("an http issuer passes the contract",
     [(CLIENT, "        value.issuer.startsWith('https://') &&\n", "")], FE_SNAPSHOT, SNAP),
    ("more saved conversations than the bound pass the contract",
     [(CLIENT, "        conversations.length + unreadable.length <= MAX_SAVED_CONVERSATIONS &&\n", "")], FE_SNAPSHOT, SNAP),
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
