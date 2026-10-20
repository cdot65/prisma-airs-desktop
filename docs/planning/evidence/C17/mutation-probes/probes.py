"""Deliberate behavior mutations for C17's reviewed Gateway changes: the
chosen workspace as the only scope (drafts naming another, items of another,
creates placed there, the review bound to the selection reference), the
tenant as the organisation, credentials never in a draft and stored ones
protected, explicit provisioning scopes, catalog parameters that fail closed,
counters that never conflict, a real change that does, references that
block deletes and archives, one-time keys taken first, provisioning outcomes
(unknown, partially completed, cleaned up) and their reconciliation, stopping
that reports not sent or unknown, readbacks that say when the reviewed change
did not take, a credential replacement that ends the review, and the
presentation and contract rules (the selection reference in a draft, Apply
disabled while referenced, provisioning steps, Stop waiting, the workspace
detail read, models bound to their integration). The remediation adds: a
workspace switch that retires an apply before its write, a stopped readback
that keeps the answered write and its key (Gateway and Runtime), the
all-models setting in a models readback, reconciliation by this attempt's
identity, the SDK's scope cleanup reported, summaries that call only what was
read back verified, and drafts and uncertain results kept on this Mac (no
credentials, nothing reviewable outside its workspace). The attempt-02
remediation adds: incomplete text never kept, a reset that fences local
reads and writes, a change in flight recorded under its own tenant, and
nothing kept for a deleted tenant.

Run in a disposable copy of the repository (never the reviewed checkout):
    python3 -I -B probes.py <copy-root> <results.json> [name filter]
Each probe applies one mutation that violates a guarded invariant, runs the
guarding target, requires it to fail on the named test, and restores the
files. Unmutated baselines of every target must pass three runs, with their
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
GW_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "gateway_changes"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "gateway_changes"]
RUNTIME_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "runtime_changes"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_GW = vitest("src/features/gateway/gateway-view.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (GW_T, UNIT_T, FE_GW, FE_SNAPSHOT)

CHANGES = "crates/desktop-core/src/gateway_changes.rs"
DOMAIN = "crates/desktop-core/src/authority/change_domain.rs"
GW_DOMAIN = "crates/desktop-core/src/authority/gateway_domain.rs"
LOCAL = "crates/desktop-core/src/gateway_local.rs"
LOCAL_DOMAIN = "crates/desktop-core/src/authority/gateway_local_domain.rs"
AUTHORITY = "crates/desktop-core/src/authority.rs"
VIEW = "apps/desktop/src/features/gateway/gateway-view.tsx"
WORKBENCH = "apps/desktop/src/features/administration/runtime-changes.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"

EDIT = "a_reviewed_change_inside_the_chosen_workspace_applies_once_after_confirmation"
BOUND = "a_review_is_bound_to_its_workspace_and_a_draft_cannot_choose_another"
CONFLICT = "a_change_made_since_review_is_a_conflict_but_traffic_counters_are_not"
REFS = "deletes_are_blocked_while_anything_references_the_item"
CREDS = "credentials_never_travel_in_a_draft_and_stored_ones_are_protected"
CATALOG = "guardrail_checks_are_validated_against_the_catalog"
KEYS = "one_time_keys_are_held_in_memory_and_shown_once"
PROVISION = "provisioning_reports_every_step_and_reconciles_what_exists"
UNCERTAIN = "an_uncertain_or_failed_step_keeps_its_outcome_and_nothing_is_retried_or_undone"
CANCEL = "cancelling_an_apply_never_claims_a_rollback"
READBACK = "readbacks_say_when_the_service_did_not_take_the_reviewed_change"
U_SCOPED = "scoped_creates_are_placed_in_the_chosen_workspace_only"
U_CREDS = "drafts_never_carry_credentials"
U_PARAMS = "catalog_parameters_are_checked_and_fail_closed"
U_KEY = "a_one_time_key_is_taken_before_anything_is_shown"
U_SCOPE = "provisioning_names_its_exact_scope"
FE_MODELS = "never shows one integration’s models under another"
FE_REVIEW = "reviews a change with the selection reference and shows its scope and what blocks it"
FE_STEPS = "reports provisioning step by step and offers to stop waiting without claiming a rollback"
FE_SPACE = "shows the chosen workspace’s own settings and limits, and its failures"
FE_CONTRACT = "rejects references, steps and states Rust never issues"
MATRIX = "every_advertised_operation_sends_its_literal_request_once_and_settles_by_outcome"
SWITCH = "choosing_another_workspace_before_the_write_sends_nothing"
STOPPED = "stopping_during_the_readback_keeps_the_answered_write_and_its_key"
ALL_MODELS = "a_models_readback_checks_the_all_models_setting"
SHARED_SCOPE = "a_reused_scope_never_attributes_another_workspace_to_the_attempt"
CLEANUP = "a_failed_cleanup_is_reported_and_never_called_undone"
OFFLINE = "drafts_and_uncertain_outcomes_survive_an_offline_restart_and_nothing_is_replayed"
TYPING = "credential_text_never_reaches_this_mac_even_while_it_is_being_typed"
RESET = "local_work_held_across_a_reset_never_reappears"
IN_FLIGHT = "a_write_in_flight_when_the_context_changes_is_kept_under_its_own_tenant"
RUNTIME_STOPPED = "stopping_during_the_readback_keeps_the_completed_write_and_its_key"
FE_VERIFIED = "calls a provisioning verified only when reading the tenant back established it"
FE_LOCAL = "keeps drafts and uncertain results on this Mac, readable offline and reviewed again only when asked"
FE_LOCAL_CONTRACT = "rejects kept work Rust never issues"

PROBES = [
    ("a draft may name another workspace", [(CHANGES, "Some(_) => return Err(ChangeProblem::WorkspaceMismatch),", "Some(_) => {}")],
     UNIT_T, U_SCOPED),
    ("a create is not placed in the chosen workspace",
     [(CHANGES, 'body.insert("workspace_id".into(), Value::from(workspace));', "let _ = workspace;")], UNIT_T, U_SCOPED),
    ("a create does not belong to the credential's tenant",
     [(CHANGES, 'body.insert("organisation_id".into(), Value::from(tsg));', "let _ = tsg;")], UNIT_T, U_SCOPED),
    ("credential-bearing drafts are accepted",
     [(CHANGES, "|| has_credentials(&Value::Object(body.clone()))", "|| false")], UNIT_T, U_CREDS),
    ("provisioning may leave its scope to be generated",
     [(CHANGES, "if !named.is_some_and(|n| scm_sdk::shared::iam::validate_scope_name(n).is_ok()) {",
       "if named.is_some_and(|n| n.is_empty()) {")], UNIT_T, U_SCOPE),
    ("catalog patterns are accepted unchecked", [(CHANGES, '"else", "pattern",', '"else",')], UNIT_T, U_PARAMS),
    ("the one-time key stays in an unstripped reply",
     [(CHANGES, '.and_then(|m| m.remove("key"))', '.and_then(|m| m.get("key").cloned())'),
      (CHANGES, "let mut reply = public_document(&reply);", "let mut reply = reply.clone();")], UNIT_T, U_KEY),
    ("an item of another workspace is changed",
     [(CHANGES, "Some(owner) if owner.as_str() != Some(scope.id.as_str()) => {", 'Some(owner) if owner.as_str() == Some("never") => {')],
     GW_T, BOUND),
    ("a stored routing credential may be replaced",
     [(CHANGES, 'if body.get("config").is_some() && stored {', 'if body.get("config").is_some() && !stored && false {')], GW_T, CREDS),
    ("stored guardrail credentials may be lost",
     [(CHANGES, "                if stored {\n                    return Err(ChangeProblem::ProtectedConfiguration.into());\n                }\n                guardrail_checks",
       "                if false {\n                    return Err(ChangeProblem::ProtectedConfiguration.into());\n                }\n                guardrail_checks")],
     GW_T, CREDS),
    ("guardrail checks skip the catalog",
     [(CHANGES, "guardrail_checks(gw, checks, before.as_ref(), o).await?;", "let _ = checks;")], GW_T, CATALOG),
    ("an existing scope is provisioned as new",
     [(CHANGES, "Ok(_) if !reuse => return Err(ChangeProblem::ScopeExists.into()),", "Ok(_) if false => return Err(ChangeProblem::ScopeExists.into()),")],
     GW_T, PROVISION),
    ("traffic counters make a review conflict",
     [(CHANGES, ".filter(|(k, _)| !VOLATILE_FIELDS.contains(&k.as_str()))", ".filter(|(k, _)| !k.is_empty())")], GW_T, CONFLICT),
    ("referencing routes are not listed",
     [(CHANGES, "if references(&detail, &ids) {", "if false && references(&detail, &ids) {")], GW_T, REFS),
    ("a non-empty workspace can be archived",
     [(CHANGES, "for row in rows(gw, collection, Some(id), options).await? {\n                    found.push(label(name, &row));",
       "for row in rows(gw, collection, Some(id), options).await? {\n                    let _ = (name, row);")], GW_T, REFS),
    ("workspaces granted the integration are ignored",
     [(CHANGES, 'if binding.get("enabled") != Some(&Value::Bool(false)) {', 'if binding.get("enabled") == Some(&Value::Bool(false)) {')],
     GW_T, REFS),
    ("model readback claims what was not taken", [(CHANGES, "if models_show(&mutation.body, &read) {", "if true {")], GW_T, READBACK),
    ("a routing readback is never checked",
     [(CHANGES, "if differs {\n                ReadBack::Differs(shown)", "if false {\n                ReadBack::Differs(shown)")], GW_T, READBACK),
    ("provisioning is not reconciled",
     [(CHANGES, "    reconcile(gw, &mut report, mutation, prior, deadline).await;\n    (report, failure)", "    let _ = prior;\n    (report, failure)")],
     GW_T, PROVISION),
    ("RequestApply ignores references", [(DOMAIN, "|| !prepared.references.is_empty()", "|| false")], GW_T, REFS),
    ("a Gateway change ignores a change since review",
     [(DOMAIN, "            return Self::failed(Outcome::NotSent, ChangeProblem::Referenced.into());\n        }\n        if digest != self.prepared.digest {",
       "            return Self::failed(Outcome::NotSent, ChangeProblem::Referenced.into());\n        }\n        if false && digest != self.prepared.digest {")],
     GW_T, CONFLICT),
    ("a review survives another workspace choice", [(DOMAIN, "if active.as_ref() != Some(&review.context) || retired {", "if active.as_ref() != Some(&review.context) {")],
     GW_T, BOUND),
    ("a review survives a context change", [(DOMAIN, "if active.as_ref() != Some(&review.context) || retired {", "if retired {")],
     RUNTIME_T, "a_denied_or_stale_confirmation_sends_nothing"),
    ("an uncertain step is reported known",
     [(DOMAIN, "Some(_) if steps.contains(&gateway::StepState::Unknown) => Outcome::Unknown,", "Some(_) if false => Outcome::Unknown,")],
     GW_T, UNCERTAIN),
    ("a cleaned-up scope counts as partial completion",
     [(DOMAIN, "Some(_) if steps.contains(&gateway::StepState::Completed) => Outcome::Completed,",
       "Some(_) if steps.contains(&gateway::StepState::Completed) || steps.contains(&gateway::StepState::Deleted) => {\n                    Outcome::Completed\n                }")],
     GW_T, UNCERTAIN),
    ("a stop before the write is reported unknown",
     [(DOMAIN, "Progress::Waiting => return Self::failed(Outcome::NotSent, cancelled),",
       "Progress::Waiting => return Self::failed(Outcome::Unknown, cancelled),")], GW_T, CANCEL),
    ("a Gateway write is not marked sent",
     [(DOMAIN, "        if !self.dispatch(prior.clone()) {\n            return Self::failed(Outcome::NotSent, ChangeFailure::WorkspaceChanged);\n        }\n",
       "        let _ = &prior;\n")], GW_T, CANCEL),
    ("a retired workspace's apply still writes",
     [(DOMAIN, "if matches!(*progress, Progress::Retired) {", "if matches!(*progress, Progress::Retired) && false {")], GW_T, SWITCH),
    ("choosing another workspace leaves its applies waiting",
     [(GW_DOMAIN, "        self.retire_scoped_applies(keep);\n", "")], GW_T, SWITCH),
    ("a stopped readback drops the answered write and its key",
     [(DOMAIN, "Progress::Written(output) => return *output,", "Progress::Written(_) => return Self::failed(Outcome::Unknown, cancelled),")],
     GW_T, STOPPED),
    ("a Runtime write is forgotten when its readback is stopped",
     [(DOMAIN, "        self.answered(&output);\n        let (readback, current) = readback(binding, mutation, before, &response, deadline).await;",
       "        let (readback, current) = readback(binding, mutation, before, &response, deadline).await;")],
     RUNTIME_T, RUNTIME_STOPPED),
    ("the all-models setting is not read back", [(CHANGES, ".is_none_or(|wanted| wanted == read.allow_all);", ".is_none_or(|_| true);")],
     GW_T, ALL_MODELS),
    ("reconciliation ignores which workspace is the attempt's",
     [(CHANGES, '.filter(|w| w["name"].as_str() == name)', ".filter(|_| name.is_some() || true)"),
      (CHANGES, "prior.is_some_and(|p| !p.iter().any(|known| known == id))", "{ let _ = (prior, id); true }")], GW_T, SHARED_SCOPE),
    ("the scope cleanup is not reported", [(CHANGES, "report.cleanup = cleanup(error.recovery());", "let _ = cleanup;")], GW_T, CLEANUP),
    ("an uncertain change is not kept on this Mac", [(DOMAIN, "Outcome::Unknown => true,", "Outcome::Unknown => false,")], GW_T, OFFLINE),
    ("a kept draft may hold a credential", [(LOCAL, "if holds_credentials(&parsed) {", "if false && holds_credentials(&parsed) {")], GW_T, OFFLINE),
    ("text still being typed is kept as a draft",
     [(LOCAL, "let parsed = serde_json::from_str::<serde_json::Value>(body)\n        .map_err(|_| GatewayDraftProblem::Incomplete)?;",
       "let Ok(parsed) = serde_json::from_str::<serde_json::Value>(body) else {\n        return Ok(());\n    };")], GW_T, TYPING),
    ("a reset leaves Gateway local work to be read and written",
     [(AUTHORITY, "            state.workbench.clear();\n            state.forget_all_gateway_local();\n", "            state.workbench.clear();\n"),
      (AUTHORITY, "                state.scans.clear();\n                state.forget_all_gateway_local();\n", "                state.scans.clear();\n"),
      (LOCAL_DOMAIN, "if self.storage == super::StorageHealth::Resetting || self.tenant(tenant).is_none() {",
       "if self.storage == super::StorageHealth::Resetting {")], GW_T, RESET),
    ("a change in flight is dropped with its context",
     [(DOMAIN, "                self.detached_changes.push(record);\n", "                drop(record);\n")], GW_T, IN_FLIGHT),
    ("a deleted tenant's change is still kept",
     [(LOCAL_DOMAIN, "        if self.tenant(tenant).is_none()\n            || self.sessions_root.is_none()", "        if self.sessions_root.is_none()"),
      (LOCAL_DOMAIN, "if self.storage == super::StorageHealth::Resetting || self.tenant(tenant).is_none() {",
       "if self.storage == super::StorageHealth::Resetting {")], GW_T, IN_FLIGHT),
    ("a provisioning is called verified without its readback",
     [(WORKBENCH, "return missing.length === 0", "return missing.length >= 0")], FE_GW, FE_VERIFIED),
    ("a kept draft can be reviewed outside its workspace",
     [(WORKBENCH, "disabled={busy || confirming || !!change.reviewBlocked}", "disabled={busy || confirming}")], FE_GW, FE_LOCAL),
    ("work kept on this Mac is never opened",
     [(VIEW, "if (context && local.state === 'idle') void dispatch", "if (context && local.state === 'never') void dispatch")], FE_GW, FE_LOCAL),
    ("kept work of other kinds is accepted", [(CLIENT, "value.kind.startsWith('gateway') &&", "true &&")], FE_SNAPSHOT, FE_LOCAL_CONTRACT),
    ("a detail keeps another integration's models", [(VIEW, "<DetailPanel key={detail.id} state={detail}", "<DetailPanel state={detail}")],
     FE_GW, FE_MODELS),
    ("the workspace detail is read as a scoped read",
     [(VIEW, "read({ kind: 'detail', resource: 'workspaces', id: selected.workspace.id }, false)",
       "read({ kind: 'detail', resource: 'workspaces', id: selected.workspace.id }, true)")], FE_GW, FE_SPACE),
    ("a draft omits the selection reference", [(WORKBENCH, "if (change.workspace) captured.workspace = change.workspace", "void change.workspace")],
     FE_GW, FE_REVIEW),
    ("Apply is offered while references remain", [(WORKBENCH, "disabled={busy || confirming || blocked}", "disabled={busy || confirming}")],
     FE_GW, FE_REVIEW),
    ("provisioning steps are not shown",
     [(WORKBENCH, "{result.provisioning ? <ProvisioningSteps p={result.provisioning} /> : null}", "{null}")], FE_GW, FE_STEPS),
    ("Stop waiting sends nothing",
     [(WORKBENCH, "onClick={() => void send({ kind: 'cancelChange', operation: result.operation })}", "onClick={() => undefined}")],
     FE_GW, FE_STEPS),
    ("references are accepted on any review",
     [(CLIENT, "(value.references.length === 0 || (value.action === 'delete' && value.state !== 'failed'))", "true")],
     FE_SNAPSHOT, FE_CONTRACT),
    ("steps are accepted on any result",
     [(CLIENT, "(value.kind === 'gatewayWorkspaces' && value.action === 'create' && isProvisioning(value.provisioning))",
       "isProvisioning(value.provisioning)")], FE_SNAPSHOT, FE_CONTRACT),
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
