"""Deliberate behavior mutations for C16's AI Gateway reads: allow-listed
projections (provider keys, key values, credential-bearing routing and
guardrail settings, limits), completeness that is never claimed from a
partial inventory, refused malformed inventories, details proven to belong
to the chosen workspace, the native workspace choice (listed only, its
current reference, retirement of the reads before it, withdrawal of a
result settled before another choice, clearing on a context change,
restore only when listed, the saved choice), prerequisites stated as read,
request validation, and the presentation and contract rules (dropped late
replies, focus return, partial coverage text, row counts, totals and
workspace references).

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
GW_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "gateway_reads"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "gateway"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_GW = vitest("src/features/gateway/gateway-view.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (GW_T, UNIT_T, FE_GW, FE_SNAPSHOT)

GATEWAY = "crates/desktop-core/src/gateway.rs"
DOMAIN = "crates/desktop-core/src/authority/gateway_domain.rs"
MGMT_DOMAIN = "crates/desktop-core/src/authority/management_domain.rs"
MANAGEMENT = "crates/desktop-core/src/management.rs"
CORE = "crates/desktop-core/src/authority.rs"
VIEW = "apps/desktop/src/features/gateway/gateway-view.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"

ADVERTISED = "every_advertised_read_sends_its_exact_get_with_the_tenant_header"
KEYS = "key_material_and_credential_bearing_configuration_stay_in_the_core"
SWITCH = "choosing_another_workspace_retires_the_reads_of_the_one_before"
SETTLED = "a_result_settled_before_another_choice_is_withdrawn_when_observed"
ADMITTED = "reads_are_admitted_only_for_listed_workspaces_and_their_current_reference"
CONTEXT = "a_context_change_clears_the_choice_which_returns_only_when_listed_again"
RESTART = "the_choice_is_remembered_across_restarts_and_forgotten_with_the_tenant"
PARTIAL = "partial_and_malformed_inventories_are_never_complete_or_empty"
DETAILS = "details_are_shown_only_when_they_belong_to_the_chosen_workspace"
PREREQ = "prerequisites_report_each_plane_and_the_scope_binding_as_read"
PROJECT_U = "projections_never_carry_key_material"
LATE = "drops a reply for a workspace chosen before"
FOCUS = "opens a detail with focus on its heading and returns focus on close"
COVERAGE = "never reads \"none\" from a partial inventory and says a complete one is empty"
CONTRACT = "rejects inventories, details and prerequisites Rust never issues"
CHOICE = "validates the workspace choice in the snapshot"
SPACE = "shows the chosen workspace’s own settings and limits, and its failures"
MODELS = "never shows one integration’s models under another"

PROBES = [
    ("a projection carries the provider key",
     [(GATEWAY, "const BASE_FIELDS: [&str; 28] = [\n    \"id\",", "const BASE_FIELDS: [&str; 29] = [\n    \"key\",\n    \"id\",")],
     GW_T, KEYS),
    ("limits are passed through whole",
     [(GATEWAY, "safe.insert(key.to_owned(), limits(v));", "safe.insert(key.to_owned(), v.clone());")],
     UNIT_T, PROJECT_U),
    ("api keys are not secret field names",
     [(GATEWAY, "            \"apikey\",\n", "")],
     GW_T, KEYS),
    ("forward headers are hidden as credentials",
     [(GATEWAY, "    secret_field(key) && !forwarding_names(key, value)\n", "    secret_field(key)\n")],
     GW_T, KEYS),
    ("has_more true is complete",
     [(GATEWAY, "        Some(Value::Bool(more)) => !more,", "        Some(Value::Bool(_)) => true,")],
     GW_T, PARTIAL),
    ("a missing has_more is complete",
     [(GATEWAY, "        _ => total.is_some_and(|t| t <= count),\n    };\n    Some(GatewayList {", "        _ => true,\n    };\n    Some(GatewayList {")],
     GW_T, PARTIAL),
    ("a total below its rows is accepted",
     [(GATEWAY, "    if total.is_some_and(|t| t < count) {\n        return None;\n    }\n    let complete", "    let complete")],
     GW_T, PARTIAL),
    ("duplicate rows are accepted",
     [(GATEWAY, "if !ids.insert(projected[\"id\"].as_str()?.to_owned()) {", "if !ids.insert(projected[\"id\"].as_str()?.to_owned()) && false {")],
     GW_T, PARTIAL),
    ("another workspace's rows are accepted",
     [(GATEWAY, "            && owner.as_str() != workspace\n", "            && owner.as_str() != workspace\n            && false\n")],
     GW_T, PARTIAL),
    ("a detail naming another workspace is shown",
     [(GATEWAY, "Some(owner) if owner.as_str() != Some(scope.id.as_str()) => {", "Some(owner) if owner.as_str() == Some(\"never\") => {")],
     GW_T, DETAILS),
    ("absence is claimed from a partial inventory",
     [(GATEWAY, "found = if inventory.complete {", "found = if true {")],
     GW_T, DETAILS),
    ("a reply for another item is shown",
     [(GATEWAY, "    if reply.get(\"id\").and_then(Value::as_str) != Some(id) {", "    if false {")],
     GW_T, DETAILS),
    ("identifiers may hold path characters",
     [(GATEWAY, "            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')\n}", "            .all(|b| b != 0)\n}")],
     GW_T, ADMITTED),
    ("a tenant-wide read may carry a workspace",
     [(MANAGEMENT, "read.is_valid() && workspace.is_some() == read.scoped()", "read.is_valid() && (workspace.is_some() || !read.scoped())")],
     GW_T, ADMITTED),
    ("a scoped read takes any reference",
     [(DOMAIN, ".filter(|s| &s.context == context && Some(&s.reference) == workspace.as_ref())", ".filter(|s| &s.context == context)")],
     GW_T, SWITCH),
    ("an unlisted workspace can be chosen",
     [(DOMAIN, ".and_then(|l| l.workspaces.iter().find(|w| w.id == workspace))", ".and_then(|l| l.workspaces.iter().find(|w| w.id == workspace).or(l.workspaces.first()))")],
     GW_T, ADMITTED),
    ("a settled result is shown after another choice",
     [(MGMT_DOMAIN, "            && self.workspace_current(workspace.as_ref());", ";")],
     GW_T, SETTLED),
    ("a context change keeps the choice",
     [(CORE, "        self.retire_management_bindings();\n        let mut starts = starts;\n        starts.extend(self.retire_gateway());\n        self.sync_scans();\n        self.sync_management_calls();\n        self.sync_changes();\n        starts\n    }\n\n    fn scan_session(",
       "        self.retire_management_bindings();\n        let starts = starts;\n        self.sync_scans();\n        self.sync_management_calls();\n        self.sync_changes();\n        starts\n    }\n\n    fn scan_session(")],
     GW_T, CONTEXT),
    ("a remembered workspace is never restored",
     [(DOMAIN, "starts.extend(self.choose(context.clone(), listed, true));", "let _ = listed;")],
     GW_T, CONTEXT),
    ("a choice no longer listed stays",
     [(DOMAIN, "None if list.complete => {", "None if false => {")],
     GW_T, CONTEXT),
    ("a choice is not saved",
     [(DOMAIN, "self.save_with(&before)?;", "let _ = &before;")],
     GW_T, RESTART),
    ("a refused plane is reported as failed",
     [(GATEWAY, "        PlaneAccess::Denied\n    } else {", "        PlaneAccess::Failed\n    } else {")],
     GW_T, PREREQ),
    ("an absent scope is reported unbound",
     [(GATEWAY, "ManagementFailure::NotFound => ScopeBinding::Absent,", "ManagementFailure::NotFound => ScopeBinding::Unbound,")],
     GW_T, PREREQ),
    ("inference keys ignore their scopes",
     [(GATEWAY, ".is_some_and(|s| s.iter().any(|s| s.as_str() == Some(INFERENCE_SCOPE)))", ".is_some()")],
     GW_T, PREREQ),
    ("a late reply for another workspace is shown",
     [(VIEW, "        if (mine !== tokens.current[resource] || gen !== generation.current) return\n", "        void mine\n        void gen\n")],
     FE_GW, LATE),
    ("focus is not returned on close",
     [(VIEW, "        setDetail(null)\n        opener.current?.focus()\n", "        setDetail(null)\n        void opener\n")],
     FE_GW, FOCUS),
    ("a partial inventory reads as complete",
     [(VIEW, "    if (list.complete) return list.count === 0", "    if (list.complete || true) return list.count === 0")],
     FE_GW, COVERAGE),
    ("rows that disagree with their count pass the contract",
     [(CLIENT, "        (JSON.parse(value.rows as string) as unknown[]).length === value.count &&\n", "")],
     FE_SNAPSHOT, CONTRACT),
    ("a total below the rows passes the contract",
     [(CLIENT, "        (value.total === null || (value.total as number) >= (value.count as number)) &&\n", "")],
     FE_SNAPSHOT, CONTRACT),
    ("any reference passes as a workspace choice",
     [(CLIENT, "                WORKSPACE_REF.test(s.reference) &&\n", "")],
     FE_SNAPSHOT, CHOICE),
    # Follow-ups R1 and R2 (reviews/C16/follow-ups.md).
    ("a detail keeps another integration's models",
     [(VIEW, "<DetailPanel key={detail.id} state={detail}", "<DetailPanel state={detail}")],
     FE_GW, MODELS),
    ("a late workspace detail is shown under another workspace",
     [(VIEW, "        if (mine !== tokens.current.workspace || gen !== generation.current) return\n", "")],
     FE_GW, SPACE),
    ("the workspace detail is never read",
     [(VIEW, "        const result = await read({ kind: 'detail', resource: 'workspaces', id: selected.workspace.id }, false)\n",
       "        const result = 'not read'\n")],
     FE_GW, SPACE),
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
