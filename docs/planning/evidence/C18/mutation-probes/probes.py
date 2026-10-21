"""Deliberate behavior mutations for C18's Gateway inference and
observation: output bounded, a run shown only under its workspace and
stopped by a new key, the key never used for another workspace, storing it
never a context change, stopping never claiming nothing was charged, logs
and policies only of the chosen workspace, a partial window claiming no
total, each usage source keeping its own failure, the overview keeping the
Gateway when Runtime fails, uncertain runs kept on this Mac, one run at a
time, and the presentation and contract rules (the charge statement, Run
only with this workspace's key, kept runs without output). Remediation
(attempt 2) adds: opening failures keep the SDK's outcome, a retired key's
run withdrawn at once, kept drafts edited offline by ID and pending edits
kept on leaving, log pages bound to their query and a capped list never
complete, Gateway holds ended only by a known day of their own workspace,
and the Responses API. Remediation (attempt 3) adds: an answer cut short
shown as incomplete (both APIs, and in the Playground), and the Gateway
files subset (a run attaches only a listed file, an upload taken once and
bounded, only listed files deleted, a new key withdrawing the files, an
uncertain upload kept and a refusal not, delete only after asking, a chosen
file carrying only its name and size, no file sent with a chat run).

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
INF_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "gateway_inference"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "gateway_observe"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_INF = vitest("src/features/gateway/gateway-inference.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (INF_T, UNIT_T, FE_INF, FE_SNAPSHOT)

INFERENCE = "crates/desktop-core/src/gateway_inference.rs"
DOMAIN = "crates/desktop-core/src/authority/inference_domain.rs"
AUTHORITY = "crates/desktop-core/src/authority.rs"
OBSERVE = "crates/desktop-core/src/gateway_observe.rs"
OVERVIEW = "crates/desktop-core/src/authority/observation_domain.rs"
LOCAL_DOMAIN = "crates/desktop-core/src/authority/gateway_local_domain.rs"
PLAYGROUND = "apps/desktop/src/features/gateway/playground.tsx"
OBSERVE_FE = "apps/desktop/src/features/gateway/gateway-observe.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"
FILES = "crates/desktop-core/src/gateway_files.rs"
FILES_DOMAIN = "crates/desktop-core/src/authority/gateway_files_domain.rs"
FILES_FE = "apps/desktop/src/features/gateway/gateway-files.tsx"

KEY = "the_inference_key_is_bound_to_its_workspace_and_endpoint_and_never_leaves_the_core"
STREAM = "a_run_streams_its_output_once_with_its_route_and_trace_and_is_never_retried"
BOUNDED = "output_is_bounded_and_a_malformed_chunk_ends_the_run"
STOP = "stopping_ends_the_stream_and_never_claims_nothing_was_charged"
RETIRED = "another_workspace_tenant_or_key_stops_the_run_and_its_late_output_is_dropped"
OBSERVED = "usage_logs_and_limits_read_only_the_chosen_workspace_and_keep_each_sources_failure"
HOME_T = "the_overview_reports_the_chosen_workspaces_gateway_day_with_its_source_state"
U_CHARTS = "charts_report_only_what_the_service_did"
U_POLICIES = "policies_name_the_chosen_workspace_and_hide_their_conditions"
UNCERTAIN = "uncertain_openings_keep_their_trace_across_a_restart_and_are_never_replayed"
WITHDRAWN = "a_replaced_or_removed_key_withdraws_its_run_from_every_snapshot_at_once"
RESTART = "playground_drafts_and_uncertain_runs_survive_an_offline_restart_and_nothing_is_replayed"
RESPONSES = "a_responses_run_streams_once_and_reports_how_it_ended"
HOLD = "a_gateway_hold_ends_only_from_a_known_day_of_its_own_workspace"
FE_LEAVE = "keeps a Playground edit made just before leaving the tab"
FE_OFFLINE = "edits a kept Playground draft offline by its ID and keeps edits made just before leaving"
FE_LOGS = "continues the logs of the query shown, never mixes queries and never calls a capped list complete"
FE_KEY = "stores the inference key for the chosen workspace and never shows it"
FE_RUN = "runs once when asked, streams the output, and stopping never claims nothing was charged"
FE_CONTRACT = "rejects observations, runs and keys Rust never issues"
FILES_T = "gateway_files_are_read_uploaded_attached_and_deleted_once_with_the_workspace_key"
FILE_KEPT = "uncertain_file_changes_are_kept_with_their_trace_and_never_resent"
FILE_WITHDRAWN = "another_key_or_workspace_withdraws_the_files_and_their_attachments"
FE_CUT = "tells a complete answer from one the Gateway cut short and from a failed one"
FE_FILES = "reads, uploads and deletes Gateway files once each, and the view never holds a path or content"
FE_ATTACH = "attaches a file the workspace listed to a Responses run, and keeps it in the draft"
FILE_BOUNDS = "file_lists_keep_their_bound_deletes_their_identity_and_times_their_range"
FE_FULL = "keeps the Files view usable with a full list after uploads and times no date can render"
CONTRACT_OK = "accepts what Rust issues"

PROBES = [
    ("output is not bounded", [(INFERENCE, "    if text.len() <= room {", "    if text.len() <= room || true {")], INF_T, BOUNDED),
    ("a run is shown under another workspace",
     [(DOMAIN, "            && self.workspace_current(Some(&run.reference))\n", "")], INF_T, RETIRED),
    ("replacing the key leaves its run going",
     [(AUTHORITY, "    // workspace choice are untouched).\n    if class == CredentialClass::Inference {\n        effects.push(Effect::Start(state.retire_inference(&tenant)));",
       "    // workspace choice are untouched).\n    if class == CredentialClass::Inference {\n        let _ = &tenant;")], INF_T, RETIRED),
    ("storing the key changes the tenant context",
     [(AUTHORITY, "    // workspace choice are untouched).\n    if class == CredentialClass::Inference {",
       "    // workspace choice are untouched).\n    if false {")], INF_T, KEY),
    ("the key is used for another workspace",
     [(DOMAIN, ".filter(|t| t.workspace.id == workspace.id && t.endpoint == destination)",
       ".filter(|t| t.endpoint == destination)")], INF_T, KEY),
    ("stopping claims nothing was sent",
     [(INFERENCE, "        RunEnd::Stopped | RunEnd::Failed if !p.sent => Outcome::NotSent,",
       "        RunEnd::Stopped => Outcome::NotSent,\n        RunEnd::Failed if !p.sent => Outcome::NotSent,")], INF_T, STOP),
    ("two runs at once", [(DOMAIN, "            return Err(Refusal::Busy);\n        }\n        let trace = trace_id()",
                           "            let _ = Refusal::Busy;\n        }\n        let trace = trace_id()")], INF_T, STREAM),
    ("another workspace's log row is shown",
     [(OBSERVE, '            if r.get("workspace_slug").and_then(Value::as_str) != Some(slug) {',
       '            if r.get("workspace_slug").and_then(Value::as_str).is_none() && slug.is_empty() {')], INF_T, OBSERVED),
    ("another workspace's policy is shown",
     [(OBSERVE, '            if p.get("workspace_id").and_then(Value::as_str) != Some(workspace) {',
       '            if p.get("workspace_id").and_then(Value::as_str).is_none() && workspace.is_empty() {')], UNIT_T, U_POLICIES),
    ("a partial window claims a total",
     [(OBSERVE, "    let total = if partial {\n        None\n    } else if latency {",
       "    let total = if false {\n        None\n    } else if latency {")], UNIT_T, U_CHARTS),
    ("one failed usage source fails them all",
     [(OBSERVE, "        .all(|r| r.is_err())\n        && models.is_err()", "        .any(|r| r.is_err())\n        || models.is_err()")], INF_T, OBSERVED),
    ("the overview drops the Gateway when Runtime fails",
     [(OVERVIEW, "                        .is_none_or(|g| g.usage_failure.is_some() && g.policies.is_none()) =>",
       "                        .is_none_or(|g| g.usage_failure.is_some() || g.policies.is_some()) =>")], INF_T, HOME_T),
    ("an uncertain run is not kept on this Mac", [(LOCAL_DOMAIN, "        if !uncertain\n", "        if true\n")], INF_T, STOP),
    ("the response says nothing about charges",
     [(PLAYGROUND, "                ? ' The Gateway may have run it and charged for it: find it in the logs by its trace.'", "                ? ''")],
     FE_INF, FE_RUN),
    ("Run is offered without this workspace's key",
     [(PLAYGROUND, "disabled={!keyHere || running ||", "disabled={running ||")], FE_INF, FE_KEY),
    ("a 5xx opening is called a refusal",
     [(INFERENCE, "            Some(scm_sdk::RemoteOutcome::Rejected) => Outcome::Rejected,\n            _ => Outcome::Unknown,",
       "            Some(scm_sdk::RemoteOutcome::Rejected) => Outcome::Rejected,\n            _ => Outcome::Rejected,")], INF_T, UNCERTAIN),
    ("a retired key's run is still shown",
     [(DOMAIN, "        !run.retired\n", "        true\n"),
      (DOMAIN, "matches!(&t.inference, ScanCapability::Ready { record, .. } if record == &run.record)",
       "matches!(&t.inference, _)")], INF_T, WITHDRAWN),
    ("a kept draft cannot be edited offline",
     [(LOCAL_DOMAIN, "            (Some(id), None) => self", "            (Some(id), Some(_)) => self")], INF_T, RESTART),
    ("leaving the Playground drops a pending edit",
     [(PLAYGROUND, "    useEffect(() => flush, [flush])",
       "    useEffect(() => () => {\n        if (timer.current) clearTimeout(timer.current)\n    }, [flush])"),
      (PLAYGROUND, "                onBlur={flush}\n                onSubmit={(e) => {\n                    e.preventDefault()\n                    if (!running) void start()",
       "                onSubmit={(e) => {\n                    e.preventDefault()\n                    if (!running) void start()")], FE_INF, FE_LEAVE),
    ("an offline draft edit names a workspace",
     [(PLAYGROUND, "        void dispatch({ kind: 'savePlaygroundDraft', context, draft: d }).then(",
       "        void dispatch({ kind: 'savePlaygroundDraft', context, workspace: 'gw-aaaaaaaaaaaaaaaaaaaaaaaa', draft: d }).then(")],
     FE_INF, FE_OFFLINE),
    ("more logs follow the edited filters",
     [(OBSERVE_FE, "void load(last.page + 1, { days: last.days, trace: last.trace, status: last.status })",
       "void load(last.page + 1, query() ?? { days, trace: null, status: null })")], FE_INF, FE_LOGS),
    ("a page of another query joins the list",
     [(OBSERVE_FE, "        if (!latest.current(mine)) return\n        if (o?.kind === 'logs' && o.logs.page === page && sameQuery(o.logs, q)) {",
       "        if (o?.kind === 'logs') {"),
      (OBSERVE_FE, "            setPages((p) => (page === 0 ? [o.logs] : p.length === page && p.every((x) => sameQuery(x, q)) ? [...p, o.logs] : p))",
       "            setPages((p) => (page === 0 ? [o.logs] : [...p, o.logs]))")],
     FE_INF, FE_LOGS),
    ("a capped list is complete", [(OBSERVE, "        capped: following && !more,", "        capped: false,")], INF_T, OBSERVED),
    ("another workspace's read ends a Gateway hold",
     [(OVERVIEW, "gateway_known.as_ref() != Some(&h.id)", "gateway_known.is_none()")], INF_T, HOLD),
    ("unreported errors end a Gateway hold", [(OBSERVE, "            && self.errors.is_some()\n", "")], INF_T, HOLD),
    ("a Responses run asks the Gateway to store it", [(INFERENCE, '"store": false,', '"store": true,')], INF_T, RESPONSES),
    ("a failed response is shown as completed",
     [(INFERENCE, '"response.failed" | "error" => Step::Failed,', '"response.failed" | "error" => Step::Going,')], INF_T, RESPONSES),
    ("an incomplete Responses answer is shown completed", [(INFERENCE, "            progress.cut = true;\n", "")], INF_T, RESPONSES),
    ("a chat answer cut at its token limit is shown completed",
     [(INFERENCE, '        progress.cut = matches!(finish, "length" | "content_filter");', "        progress.cut = false;")], INF_T, RESPONSES),
    ("the Playground calls an incomplete answer completed",
     [(PLAYGROUND, "    incomplete: 'Incomplete: the Gateway ended the answer before it was finished. The output below is partial.',",
       "    incomplete: 'Completed.',")], FE_INF, FE_CUT),
    ("a run attaches a file the workspace did not list",
     [(DOMAIN, "        if let Some(file) = &run.file\n            && !self.file_listed(&reference, &record, file)\n",
       "        if let Some(_file) = &run.file\n            && false\n")], INF_T, FILES_T),
    ("an upload is sent again",
     [(FILES_DOMAIN, "files.chosen.take()",
       "files.chosen.as_ref().map(|c| Chosen { token: c.token.clone(), name: c.name.clone(), bytes: c.bytes.clone() })")],
     INF_T, FILES_T),
    ("a file over the bound is held for upload",
     [(FILES_DOMAIN, "            Some(Ok((_, bytes))) if bytes.len() > MAX_UPLOAD_BYTES => {",
       "            Some(Ok((_, bytes))) if bytes.len() > MAX_UPLOAD_BYTES * 2 => {")], INF_T, FILES_T),
    ("an unlisted file is deleted",
     [(FILES_DOMAIN, "        if !self.file_listed(&reference, &admission.record, file) {", "        if false {")], INF_T, FILES_T),
    ("a replaced key leaves the files shown",
     [(FILES_DOMAIN, "        !files.retired\n", "        true\n"),
      (FILES_DOMAIN, "matches!(&t.inference, super::ScanCapability::Ready { record, .. } if record == &files.record)",
       "matches!(&t.inference, _)")], INF_T, FILE_WITHDRAWN),
    ("an uncertain upload is not kept",
     [(FILES_DOMAIN, "                    if progress.sent && outcome == Outcome::Unknown {", "                    if false {")], INF_T, FILE_KEPT),
    ("a refused upload is called uncertain",
     [(FILES, "        scm_sdk::RemoteOutcome::Rejected => Outcome::Rejected,\n        _ => Outcome::Unknown,",
       "        _ => Outcome::Unknown,")], INF_T, FILE_KEPT),
    ("a file is deleted without asking",
     [(FILES_FE, "onClick={() => setConfirming(f)}",
       "onClick={() => void send({ kind: 'deleteGatewayFile', context, workspace: reference, file: f.id }, '')}")], FE_INF, FE_FILES),
    ("a chosen file may carry more than its name and size",
     [(CLIENT, "                Object.keys(c).length === 3,", "                true,")], FE_SNAPSHOT, FE_CONTRACT),
    ("a chat run carries the attached file",
     [(PLAYGROUND, "        if (api === 'responses' && file) r.file = file", "        if (file) r.file = file")], FE_INF, FE_ATTACH),
    # Attempt-03 remediation (reviews/C18/remediation-attempt-03.md).
    ("an upload grows the list past its bound",
     [(FILES_DOMAIN, "                            if list.len() > MAX_FILES {", "                            if false {")], INF_T, FILE_BOUNDS),
    ("an upload into a full list hides what it drops",
     [(FILES_DOMAIN, "                                *more = true;\n", "")], INF_T, FILE_BOUNDS),
    ("a delete answered for another file settles this one",
     [(FILES, "                    if &r.id == id {\n", "                    if true {\n")], INF_T, FILE_BOUNDS),
    ("a time no date can render refuses the list",
     [(FILES, "        created_at: created(value.get(\"created_at\"))?,",
       "        created_at: created(value.get(\"created_at\")).flatten().map(Some)?,")], INF_T, FILE_BOUNDS),
    ("a time no date can render throws in the Files view",
     [(FILES_FE, "    if (!Number.isFinite(at)) return 'Unknown time'\n", "")], FE_INF, FE_FULL),
    ("the contract accepts a time no date can render",
     [(CLIENT, "optional((c) => isCount(c) && (c as number) <= MAX_CREATED_AT, value.createdAt)", "optional(isCount, value.createdAt)")],
     FE_SNAPSHOT, FE_CONTRACT),
    ("the contract refuses a file with an unknown time",
     [(CLIENT, "optional((c) => isCount(c) && (c as number) <= MAX_CREATED_AT, value.createdAt)", "isCount(value.createdAt)")],
     FE_SNAPSHOT, CONTRACT_OK),
    ("a kept run may carry its output",
     [(CLIENT, "                // Kept output never: only its size.\n                !('output' in r)", "                true")], FE_SNAPSHOT, FE_CONTRACT),
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
