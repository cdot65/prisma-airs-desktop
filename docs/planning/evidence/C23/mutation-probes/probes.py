"""Deliberate behavior mutations for C23's reviewed v1 import: v1 read
without following links, past its bounds or while it runs; unknown
versions refused; protected items read only when selected; endpoints
mapped only to v2's destinations; drafts only for the same account; an
existing tenant only of the same TSG; conflicts never overwritten; every
write noted durably before it is made, and nothing written on a receipt
that is not recorded; nothing shown before the import commits; an import
that fails or that the app ended during undone whole (at the next launch,
before anything else writes) and identical work added by anyone else never
claimed; drafts removed only for their account and only while unchanged;
a changed source refused; a rollback asked for, removing only what is
unchanged and never a tenant still in use; reset removing receipts; and
the presentation and contract rules.

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
IR_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "imports_rollback"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "import_v1"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_I = vitest("src/features/import-v1/import-view.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (IR_T, UNIT_T, FE_I, FE_SNAPSHOT)

SOURCE = "crates/desktop-core/src/import_v1/source.rs"
PARSE = "crates/desktop-core/src/import_v1/parse.rs"
PLAN = "crates/desktop-core/src/import_v1/plan.rs"
DOMAIN = "crates/desktop-core/src/authority/import_v1_domain.rs"
OWNERS = "crates/desktop-core/src/authority/import_v1_domain/owners.rs"
AUTHORITY = "crates/desktop-core/src/authority.rs"
LIBRARY = "crates/desktop-core/src/authority/library_domain.rs"
VIEW = "apps/desktop/src/features/import-v1/import-view.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"

INSPECT = "inspection_reads_files_only_and_the_keychain_only_for_selected_categories"
IMPORT = "an_import_brings_work_without_credentials_and_a_rollback_removes_only_it"
TWICE = "importing_twice_finds_everything_already_present_and_owns_nothing"
CONFLICTS = "conflicts_are_reported_and_never_overwritten"
DAMAGED = "damaged_unsupported_linked_and_oversized_sources_block_only_their_category"
LOCKED = "a_running_v1_or_a_locked_keychain_blocks_without_fallback"
CHANGED = "a_source_changed_after_review_is_not_imported"
INTERRUPTED = "an_import_interrupted_anywhere_shows_nothing_and_is_undone_at_the_next_launch"
IDENTICAL = "an_identical_case_added_after_an_interruption_is_never_claimed"
OPEN_LIBRARY = "an_open_library_shows_the_previous_generation_until_the_import_commits"
NOT_RECORDED = "a_receipt_that_is_not_durably_recorded_authorizes_no_write"
ACCOUNTS = "drafts_belong_to_the_planned_account_through_import_and_rollback"
DECLINED = "a_declined_rollback_changes_nothing"
RESET = "reset_waits_for_an_import_and_removes_its_receipts"
DRAFTS = "academy_drafts_go_only_to_the_same_signed_in_account"
U_LINKS = "reads_are_bounded_and_never_follow_symlinks"
FE_REVIEW = "reads nothing until asked, then reviews the chosen categories and tenants"
FE_READY = "imports only ready items, states every status as text and focuses the result"
SNAP = "rejects categories, statuses, receipts and bounds Rust never issues"
FE_UNSETTLED = "offers Roll back for an unsettled import and blocks a new review meanwhile"

PROBES = [
    ("a link is followed",
     [(SOURCE, "    if !meta.is_file() {\n        return Err(Problem::PathRejected);\n    }\n    if meta.len() > max {", "    if meta.len() > max {"),
      (SOURCE, "        .custom_flags(O_NOFOLLOW)\n        .open(&path)\n        .map_err(|e| match e.kind() {", "        .open(&path)\n        .map_err(|e| match e.kind() {")], IR_T, DAMAGED),
    ("a running v1 is read",
     [(SOURCE, "        Err(std::fs::TryLockError::WouldBlock) => Err(Problem::SourceLocked),",
       "        Err(std::fs::TryLockError::WouldBlock) => Ok(SourceLock { _file: None }),")], IR_T, LOCKED),
    ("an oversized item is read",
     [(SOURCE, "        Ok(Some(bytes)) if bytes.len() > max => Err(Problem::TooLarge),", "        Ok(Some(bytes)) if false => Err(Problem::TooLarge),")], IR_T, DAMAGED),
    ("an unknown reports version is read",
     [(PARSE, "        Some(_) => return Err(Problem::UnsupportedVersion),\n        None => return Err(Problem::Corrupt),\n    }\n    let book: V1ReportBook",
       "        Some(_) => {}\n        None => return Err(Problem::Corrupt),\n    }\n    let book: V1ReportBook")], IR_T, DAMAGED),
    ("conversations are read unselected",
     [(PLAN, "        if selected(Category::Conversations) {", "        if true {")], IR_T, INSPECT),
    ("an endpoint is not mapped to its region",
     [(PLAN, "        Region::Europe,\n", "")], IR_T, IMPORT),
    ("another account's drafts are read",
     [(PLAN, "    if !dirs.contains(&owner_digest) {", "    if dirs.is_empty() {")], IR_T, DRAFTS),
    ("another TSG's tenant is a target",
     [(DOMAIN, "                    if !self.tsg_matches(view.tsg_id.as_deref()).contains(&tenant) {",
       "                    if self.tsg_matches(view.tsg_id.as_deref()).contains(&tenant) && false {")], IR_T, CONFLICTS),
    ("a conflicting report is overwritten",
     [(OWNERS, "            if book\n                .reports\n                .iter()\n                .any(|e| e.scan_id == r.scan_id && e.scan_sub_req_id == r.scan_sub_req_id)\n            {\n                continue;\n            }\n", "")],
     IR_T, CONFLICTS),
    ("an edited case is rolled back",
     [(OWNERS, "                        Some(c) if content_hash(c) != hash => (Revert::Kept, false),",
       "                        Some(c) if content_hash(c) != hash && false => (Revert::Kept, false),")], IR_T, IMPORT),
    ("a tenant still holding work is removed",
     [(OWNERS, "    if !cases(inner, &tenant).await?.is_empty() || !reports(inner, &tenant).await?.is_empty() {\n        return Ok(false);\n    }\n", "")], IR_T, IMPORT),
    ("a write is not noted before it is made",
     [(OWNERS, "        stage(&worker, &entries);\n        journal\n            .writing(&worker, &entries)\n            .map_err(|stop| (Vec::new(), stop))?;\n        crate::library::store(",
       "        stage(&worker, &entries);\n        crate::library::store(")], IR_T, INTERRUPTED),
    ("an unconfirmed receipt authorizes writes",
     [(DOMAIN, "    receipt::store(root, r).map_err(|_| Problem::NotRecorded)",
       "    receipt::store(root, r)\n        .or_else(|e| match e {\n            receipt::ReceiptFailure::Unconfirmed => Ok(()),\n            e => Err(e),\n        })\n        .map_err(|_| Problem::NotRecorded)")], IR_T, NOT_RECORDED),
    ("an uncommitted tenant is shown",
     [(AUTHORITY, "                .filter(|t| !self.v1_hides_tenant(&t.profile.id))\n", "")], IR_T, INTERRUPTED),
    ("an uncommitted case is shown",
     [(LIBRARY, "                    .filter(|c| !self.v1_hides_case(&l.tenant, &c.id))\n", "")], IR_T, OPEN_LIBRARY),
    ("what an import writes does not wait",
     [(DOMAIN, "pub(super) fn waits_for_import(intent: &Intent) -> bool {\n    matches!(",
       "pub(super) fn waits_for_import(intent: &Intent) -> bool {\n    false && matches!(")], IR_T, INTERRUPTED),
    ("an import the app ended during waits for the operator",
     [(DOMAIN, "        let lease = self.v1_import.recovery.take()?;",
       "        let _lease = self.v1_import.recovery.take()?;\n        let lease = None?;")], IR_T, INTERRUPTED),
    ("a failed import is left partly written",
     [(DOMAIN, "    let problem = revert(inner, root, r, true).await;\n    changed(inner, |s| {\n        s.settle_v1(lease, Some(problem.unwrap_or(failure)))",
       "    changed(inner, |s| {\n        s.put_v1_receipt(&r);\n        s.settle_v1(lease, Some(failure))")], IR_T, CHANGED),
    ("identical work added by the operator is claimed",
     [(OWNERS, "        let fresh: Vec<_> = wanted\n            .into_iter()\n            .filter(|c| !library.cases.iter().any(|e| e.id == c.id))\n            .collect();",
       "        let fresh: Vec<_> = wanted.into_iter().collect();")], IR_T, IDENTICAL),
    ("another account's draft is rolled back",
     [(OWNERS, "            if &digest != account {\n                return Revert::Failed;\n            }\n", "")], IR_T, ACCOUNTS),
    ("an edited draft is rolled back",
     [(OWNERS, "                        draft_hash(v) == hash && v[\"importedBy\"] == Value::String(receipt.clone())",
       "                        !hash.is_empty() || v[\"importedBy\"] == Value::String(receipt.clone())")], IR_T, ACCOUNTS),
    ("a changed source is imported",
     [(DOMAIN, "            && now.ok().as_deref() != Some(source_hash.as_str())",
       "            && now.ok().as_deref() == Some(\"never\")")], IR_T, CHANGED),
    ("a rollback is not asked for",
     [(DOMAIN, "    let accepted = inner.adapters.confirmer.confirm(nonce, request).await;",
       "    let accepted = {\n        drop(inner.adapters.confirmer.confirm(nonce, request));\n        true\n    };")], IR_T, DECLINED),
    ("reset leaves receipts",
     [(AUTHORITY, "                        if let Some(root) = &worker.adapters.import_root\n                            && !crate::import_v1::receipt::purge(root)\n",
       "                        if let Some(root) = &worker.adapters.import_root\n                            && root.as_os_str().is_empty()\n")], IR_T, RESET),
    ("a linked file is read (unit)",
     [(SOURCE, "    if !meta.is_file() {\n        return Err(Problem::PathRejected);\n    }\n    if meta.len() > max {", "    if meta.len() > max {"),
      (SOURCE, "        .custom_flags(O_NOFOLLOW)\n        .open(&path)\n        .map_err(|e| match e.kind() {", "        .open(&path)\n        .map_err(|e| match e.kind() {")], UNIT_T, U_LINKS),
    ("the Keychain note is never shown",
     [(VIEW, "{categories.some((c) => PROTECTED.includes(c)) ? (", "{false ? (")], FE_I, FE_REVIEW),
    ("a blocked item can be chosen",
     [(VIEW, "disabled={busy || item.status.kind !== 'ready'}", "disabled={busy}")], FE_I, FE_READY),
    ("a receipt with an unknown state passes the contract",
     [(CLIENT, "        oneOf(RECEIPT_STATES, value.state) &&\n", "")], FE_SNAPSHOT, SNAP),
    ("an unrecorded receipt's reason fails the contract",
     [(CLIENT, "    'noTarget',\n    'notRecorded',\n]", "    'noTarget',\n]")], FE_I, FE_UNSETTLED),
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
