"""Deliberate behavior mutations for C15's API Client execution: native
admission of tenant credentials (catalogue operations, token and request
destinations, placement, ambiguity, mixing, HTTPS, plain paths, the
loopback-only network), reviews (digest, consumption, native
confirmation), redirects and token handling, uncertainty, the four-request
bound, bounded previews, redaction, sealed bodies, interrupted history and
cookie scope; and the presentation and contract rules (fresh capture, the
active context, uncertain outcomes, review IDs and the running bound).

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
EX_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "workbench_execution"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "workbench"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_SEND = vitest("src/features/workbench/request-send.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (EX_T, UNIT_T, FE_SEND, FE_SNAPSHOT)

ADMISSION = "crates/desktop-core/src/workbench/admission.rs"
EXECUTE = "crates/desktop-core/src/workbench/execute.rs"
HISTORY = "crates/desktop-core/src/workbench/history.rs"
COOKIES = "crates/desktop-core/src/workbench/cookies.rs"
SEND = "crates/desktop-core/src/authority/workbench_domain/send.rs"
CORE = "crates/desktop-core/src/authority.rs"
EDITOR = "apps/desktop/src/features/workbench/document-editor.tsx"
VIEW = "apps/desktop/src/features/workbench/workbench-view.tsx"
PANELS = "apps/desktop/src/features/workbench/request-send.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"

KNOWN = "known_reads_pass_known_mutations_need_review_and_others_are_denied"
PLACED = "tenant_credentials_go_only_to_their_destinations"
ORDINARY = "ordinary_requests_keep_their_class_and_the_network_mode"
TENANT = "tenant_credentials_are_admitted_before_any_secret_is_read"
REVIEW = "known_tenant_changes_need_an_exact_confirmed_review_and_are_sent_once"
REDIRECTS = "redirects_follow_origin_rules_and_credentials_never_cross"
LOST = "lost_responses_cancels_and_the_bound_never_resend"
LARGE = "large_responses_stream_to_sealed_storage_and_save_exactly"
HISTORY_T = "history_survives_restarts_and_interrupted_runs_are_never_replayed"
CANARY = "canary_secrets_never_reach_records_or_private_files"
RESET = "reset_retires_running_sends_and_nothing_they_held_is_kept"
PLANNING = "reset_during_planning_retires_the_send_before_anything_goes"
FRESH = "sends_the_fresh_snapshot_with_v1_auth_bodies_and_cookies"
CONTRACT = "rejects runs, reviews and refusals Rust never issues"
REGISTERED_U = "registered_origins_are_classified_whatever_the_credential"
AUTHORITY_U = "host_overrides_and_url_user_info_are_refused"
REGISTERED = "registered_prisma_origins_are_classified_whatever_the_credential"
RETIRE = "a_tenant_change_retires_reviews_and_unsent_requests"
WIRE = "wire_credentials_are_redacted_wherever_runs_are_kept"
OFFLINE = "offline_then_reconnected_sends_nothing_by_itself"
AUTHORITY = "crates/desktop-core/src/authority.rs"
DOMAIN = "crates/desktop-core/src/authority/workbench_domain.rs"
OVERRIDES = "generated_overrides_and_extra_credentials_never_reach_a_registered_origin"
OVERRIDES_U = "generated_headers_overrides_and_extra_credentials_are_refused_at_registered_origins"
BOUND = "reviews_are_bound_to_the_credentials_and_cookies_actually_sent"

PROBES = [
    # ---- attempt-01 remediation (R1-R4, T23) ----
    ("registered origins are classified only for tenant credentials",
     [(ADMISSION, "        let operation = match destinations.registered.classes(&url) {",
       "        let operation = match None::<&BTreeSet<OriginClass>> {")],
     EX_T, REGISTERED),
    ("registered origins are known only with a tenant",
     [(SEND, "            return Ok((\n                Destinations {\n                    registered,\n",
       "            return Ok((\n                Destinations {\n                    registered: Registered::default(),\n")],
     EX_T, REGISTERED),
    ("ambiguous spellings at registered origins are classified",
     [(ADMISSION, "    classes: &BTreeSet<OriginClass>,\n) -> Result<Operation, SendRefusal> {\n    if !plan.url.plain().is_some_and(|t| plain_path(&t)) {",
       "    classes: &BTreeSet<OriginClass>,\n) -> Result<Operation, SendRefusal> {\n    if false {")],
     UNIT_T, REGISTERED_U),
    ("generic redirects enter registered origins",
     [(EXECUTE, "                !transfer.registered.contains(next)", "                true")],
     EX_T, REGISTERED),
    ("Host overrides reach the tenant",
     [(ADMISSION, ".any(|n| HOST_OVERRIDES.contains(&n.as_str()))", ".any(|_| false)")],
     UNIT_T, AUTHORITY_U),
    ("URL user-info rides with tenant credentials",
     [(ADMISSION, "    !url.username().is_empty()\n        || url.password().is_some()\n        || effective_headers(plan)", "    effective_headers(plan)")],
     EX_T, TENANT),
    ("a context change leaves the review usable",
     [(AUTHORITY, "        self.workbench.sending.retire(self.active_ref.as_ref());\n", "")],
     EX_T, RETIRE),
    ("unsent requests go after a context change",
     [(SEND, "    !planned.context_bound()\n        || state\n            .send_destinations", "    true\n        || state\n            .send_destinations")],
     EX_T, RETIRE),
    ("minted bearer tokens are shown",
     [(EXECUTE, "        redactions.push(token.clone());\n", "")],
     EX_T, WIRE),
    ("Basic credentials are shown",
     [(EXECUTE, "            derived.push(encoded);\n", "")],
     EX_T, WIRE),
    ("unknown managed operations are admitted",
     [(ADMISSION, "    let operation =\n        classify(&plan.method, class, url.path()).ok_or(SendRefusal::UnknownEndpoint)?;",
       "    let operation = classify(&plan.method, class, url.path()).unwrap_or(Operation {\n        index: 0,\n        name: String::new(),\n        method: plan.method.clone(),\n        mutation: false,\n    });")],
     UNIT_T, KNOWN),
    ("token URLs are not checked",
     [(ADMISSION, ".any(|admitted| url::Url::parse(admitted).ok().as_ref() == Some(&token))", ".any(|_| true)")],
     UNIT_T, PLACED),
    ("known changes need no review",
     [(ADMISSION, "            mutation: p.method != \"GET\",", "            mutation: false,")],
     EX_T, REVIEW),
    ("tenant credentials go over HTTP",
     [(ADMISSION, "    if url.scheme() != \"https\" {", "    if false {")],
     UNIT_T, PLACED),
    ("tenant credentials go anywhere in the request",
     [(ADMISSION, "    if elsewhere || managed.iter().any(|r| !placed.contains(r)) || (oauth && scan) {", "    if false {")],
     UNIT_T, PLACED),
    ("operator auth headers ride with tenant credentials",
     [(ADMISSION, "    if ambiguous || ambiguous_authority(plan, &url) {", "    if ambiguous_authority(plan, &url) {")],
     UNIT_T, PLACED),
    ("workbench secrets mix with tenant credentials",
     [(ADMISSION, "    if !managed.is_empty() && workbench {", "    if false {")],
     UNIT_T, PLACED),
    ("ambiguous path spellings are classified",
     [(ADMISSION, "    // The URL as written (before normalization) must be one plain spelling.\n    if !plan.url.plain().is_some_and(|t| plain_path(&t)) {",
       "    // The URL as written (before normalization) must be one plain spelling.\n    if false {")],
     UNIT_T, KNOWN),
    ("loopback-only builds reach named hosts",
     [(ADMISSION, "                Some(url::Host::Domain(name)) => name == \"localhost\",", "                Some(url::Host::Domain(_)) => true,")],
     UNIT_T, ORDINARY),
    ("tenant redirects are followed",
     [(EXECUTE, "                CredentialClass::Managed => false,", "                CredentialClass::Managed => true,")],
     EX_T, TENANT),
    ("workbench secrets follow cross-origin redirects",
     [(EXECUTE, "                CredentialClass::Workbench => same_origin(next, &prepared.url),", "                CredentialClass::Workbench => true,")],
     EX_T, REDIRECTS),
    ("a 401 keeps the token",
     [(EXECUTE, "        transfer.tokens.invalidate(&grant.key());\n", "")],
     EX_T, TENANT),
    ("a lost change looks certain",
     [(EXECUTE, "                ended.uncertain =\n                    mutation && !matches!(failure, RunFailure::Connect | RunFailure::Tls);",
       "                ended.uncertain = false;")],
     EX_T, LOST),
    ("the preview is unbounded",
     [(EXECUTE, "                    let take = (history::PREVIEW_BYTES - ended.preview.len()).min(chunk.len());", "                    let take = chunk.len();")],
     EX_T, LARGE),
    ("response headers keep secrets",
     [(EXECUTE, "                redact(&redactions, &text)\n                    .chars()", "                text\n                    .chars()")],
     EX_T, CANARY),
    ("body files are unbounded",
     [(EXECUTE, "    if meta.len() > limit {", "    if false {")],
     EX_T, FRESH),
    ("a reviewed file that changed still sends",
     [(EXECUTE, "&& hex::encode(hash.finalize()) != expected", "&& false && hex::encode(hash.finalize()) != expected")],
     UNIT_T, "a_pinned_file_that_changed_never_sends_its_last_piece"),
    ("a review ignores its digest",
     [(SEND, "    if pending.digest != digest\n", "    if false\n")],
     EX_T, REVIEW),
    ("a consumed review is not recognised",
     [(SEND, "    if sending.consumed.contains(review) {", "    if false {")],
     EX_T, REVIEW),
    ("Apply needs no confirmation",
     [(SEND, "    if !inner\n        .adapters\n        .confirmer\n        .confirm(u64::from_be_bytes(nonce), request)\n        .await\n    {",
       "    let _ = request;\n    if false {")],
     EX_T, REVIEW),
    ("more than four requests run",
     [(SEND, "        if state.workbench.sending.runs.len() >= MAX_ACTIVE {", "        if false {")],
     EX_T, LOST),
    ("interrupted runs stay running",
     [(HISTORY, "        r.state = RunState::Interrupted;\n", "")],
     EX_T, HISTORY_T),
    ("bodies are stored in the clear",
     [(HISTORY, "        self.file.write_all(&sealed)?;", "        let _ = &sealed;\n        self.file.write_all(&self.buffer)?;")],
     UNIT_T, "bodies_are_sealed_frames_that_cannot_be_cut_or_moved"),
    ("cookies reach parent domains",
     [(COOKIES, "                && !domain.trim_start_matches('.').eq_ignore_ascii_case(&host)", "                && false")],
     UNIT_T, "cookies_are_host_only_secure_and_path_scoped"),
    ("secure cookies go over HTTP",
     [(COOKIES, "                    && (!c.secure || https)\n", "")],
     UNIT_T, "cookies_are_host_only_secure_and_path_scoped"),
    ("Send sends the loaded fields",
     [(EDITOR, "        const snapshot = fieldsRef.current\n        setSending(true)", "        const snapshot = doc.fields\n        setSending(true)")],
     FE_SEND, "sends the fields typed before the protection debounce, with the active context"),
    ("the active context is not sent",
     [(VIEW, "                                        context={snapshot.active?.context ?? null}", "                                        context={null}")],
     FE_SEND, "sends the fields typed before the protection debounce, with the active context"),
    ("uncertain outcomes are not shown",
     [(PANELS, "            {record.uncertain ? (", "            {false ? (")],
     FE_SEND, "explains refusals and uncertain outcomes, and never offers to resend"),
    ("forged review IDs pass the contract",
     [(CLIENT, "        typeof value.review === 'string' &&\n        REVIEW_ID.test(value.review) &&\n        typeof value.request === 'string' &&",
       "        typeof value.review === 'string' &&\n        typeof value.request === 'string' &&")],
     FE_SNAPSHOT, CONTRACT),
    ("more than four running requests pass the contract",
     [(CLIENT, "        value.running.length <= 4 &&\n", "")],
     FE_SNAPSHOT, CONTRACT),
    # P04 push: the C15 review's R2 (the effective header set at registered
    # origins) and R5 (reviews bound to the credentials and cookies sent).
    ("generated auth headers are not admitted",
     [(ADMISSION, "        } => names.push(name.to_ascii_lowercase()),", "        } => names.push(name[..0].to_owned()),")],
     EX_T, OVERRIDES),
    ("method overrides reach a registered origin",
     [(ADMISSION, "        || names.iter().any(|n| METHOD_OVERRIDES.contains(&n.as_str()))\n", "")],
     EX_T, OVERRIDES),
    ("a second credential reaches a registered origin",
     [(ADMISSION, "        || credential_sources(plan) > 1\n", "")],
     EX_T, OVERRIDES),
    ("an API key in the query is not a credential",
     [(ADMISSION, "        + usize::from(!matches!(plan.auth, Auth::None))\n", "        + usize::from(!matches!(plan.auth, Auth::None | Auth::ApiKey { query: true, .. }))\n")],
     UNIT_T, OVERRIDES_U),
    ("an API key under its own header name is not a credential",
     [(ADMISSION, "        + usize::from(!matches!(plan.auth, Auth::None))\n", "        + usize::from(!matches!(plan.auth, Auth::None | Auth::ApiKey { query: false, .. }))\n")],
     UNIT_T, OVERRIDES_U),
    ("Basic usernames are not reviewed",
     [(SEND, 'serde_json::json!(["basic", username.bound(), secret])', 'serde_json::json!(["basic", secret, username.0.len() * 0])')],
     EX_T, BOUND),
    # A secret reference is bound twice, in the auth entry and as a key of
    # the secret-generation map, so dropping it from one is not observable;
    # the generation probe below removes the map.
    ("secret generations are not reviewed",
     [(SEND, '        "secrets": p.credentials.secrets,\n', '')],
     EX_T, BOUND),
    ("cookies are not reviewed",
     [(SEND, '        "cookie": cookie,\n', '        "cookie": cookie.map(|_| 0),\n')],
     EX_T, BOUND),
    ("a replaced secret keeps its generation",
     [(DOMAIN, "            .secret_changed(&collection, &reference);", "            .review();")],
     EX_T, BOUND),
    # P04 push, attempt-03 R2 (remaining): the jar's Cookie is a credential.
    ("the jar cookie is not a credential at a registered origin",
     [(SEND, "        && admission::jar_cookie_is_ambiguous(&plan)\n", "        && false\n")],
     EX_T, "a_jar_cookie_is_a_credential_at_a_registered_origin"),
    ("the jar cookie counts only beside nothing",
     [(ADMISSION, "    credential_sources(plan) > 0\n}", "    credential_sources(plan) > 1\n}")],
     EX_T, "a_jar_cookie_is_a_credential_at_a_registered_origin"),
    # P04 phase attempt-01 R1: reset fences running sends.
    ("reset purges while a storage step runs",
     [(CORE, "                        Some(fence) => Some(fence.write().await),", "                        Some(fence) => fence.try_write().ok(),")],
     EX_T, RESET),
    ("a fenced step ignores the reset epoch",
     [(SEND, "    if self::epoch(inner) != epoch {\n        return None;\n    }\n    tokio::task::spawn_blocking(f)", "    tokio::task::spawn_blocking(f)")],
     EX_T, RESET),
    ("a retired run republishes its record",
     [(SEND, "        if state.workbench.sending.epoch != planned.epoch {\n            return retired();\n        }\n        let runs = history_of(&mut state.workbench.sending, &planned.scope, Vec::new);\n        if let Some(slot)", "        let runs = history_of(&mut state.workbench.sending, &planned.scope, Vec::new);\n        if let Some(slot)")],
     EX_T, RESET),
    ("reset leaves the send epoch",
     [(SEND, "    pub(super) fn clear(&mut self) {\n        self.epoch += 1;\n", "    pub(super) fn clear(&mut self) {\n")],
     EX_T, RESET),
    # P04 phase attempt-02 R1: a Send keeps the epoch it was admitted in.
    ("planning samples the epoch after its awaits",
     [(SEND, "    let (destinations, managed, network) = {\n", "    let epoch = self::epoch(inner);\n    let (destinations, managed, network) = {\n")],
     EX_T, PLANNING),
    ("a Send is admitted in the epoch it plans in",
     [(SEND, "    let admitted_in = epoch(inner);\n", "    let admitted_in = u64::MAX;\n")],
     EX_T, "sends_the_fresh_snapshot_with_v1_auth_bodies_and_cookies"),
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
