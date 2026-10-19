"""Deliberate behavior mutations for C12's case Library and Runtime
Home/Inbox: product rules and limits, import tenant binding and merging,
catalog deduplication and URL rules, sealed storage, removal with the tenant
and on reset, unreadable libraries kept aside, copies as new cases, Runtime
summaries without fabricated zeros, key expiry, Inbox order, attention holds,
failed refreshes, and the presentation and contract rules.

Run in a disposable copy of the repository (never the reviewed checkout):
    python3 -I -B probes.py <copy-root> <results.json> [name filter]
Each probe applies one mutation that violates a guarded invariant, runs the
guarding target, requires it to fail on the named test, and restores the
files. Unmutated baselines of every target must pass.
"""
import json
import os
import subprocess
import sys

REPO = sys.argv[1]
HOME = os.environ["HOME"]
ENV = {"HOME": HOME, "PATH": f"{REPO}/.tools/bin:{HOME}/.cargo/bin:{HOME}/.nvm/versions/node/v24.21.0/bin:/usr/bin:/bin",
       "RUSTUP_TOOLCHAIN": "1.99.0", "CI": "1", "NO_COLOR": "1", "CARGO_TERM_COLOR": "never",
       "COREPACK_ENABLE_DOWNLOAD_PROMPT": "0", "TMPDIR": os.environ.get("TMPDIR", "/tmp")}
LO_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "library_observation"]
LIB_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "library"]
OBS_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "observation"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_LIBRARY = vitest("src/features/library/library-view.test.tsx")
FE_OVERVIEW = vitest("src/features/runtime/overview-inbox.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (LO_T, LIB_T, OBS_T, FE_LIBRARY, FE_OVERVIEW, FE_SNAPSHOT)

LIBRARY = "crates/desktop-core/src/library.rs"
OBS = "crates/desktop-core/src/observation.rs"
LDOMAIN = "crates/desktop-core/src/authority/library_domain.rs"
ODOMAIN = "crates/desktop-core/src/authority/observation_domain.rs"
AUTHORITY = "crates/desktop-core/src/authority.rs"
LVIEW = "apps/desktop/src/features/library/library-view.tsx"
OVIEW = "apps/desktop/src/features/runtime/overview-inbox.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"

PROBES = [
    ("a specification keeps a profile",
     [(LIBRARY, "            profile: if runtime { self.profile } else { String::new() },", "            profile: self.profile,")],
     LO_T, "cases_are_authored_sealed_and_survive_a_restart"),
    ("a 33rd case is accepted",
     [(LIBRARY, "        if self.cases.len() >= CASE_LIMIT {", "        if self.cases.len() > CASE_LIMIT {")],
     LO_T, "limits_payloads_and_unknown_types_are_refused"),
    ("a Runtime case needs no profile",
     [(LIBRARY, "            && !(runtime && self.profile.trim().is_empty())\n", "")],
     LO_T, "limits_payloads_and_unknown_types_are_refused"),
    ("another tenant's export imports",
     [(LIBRARY, "    if !(1..=2).contains(&file.version) || file.tenant != tenant || file.cases.len() > CASE_LIMIT {",
       "    if !(1..=2).contains(&file.version) || file.cases.len() > CASE_LIMIT {")],
     LO_T, "exports_and_imports_are_explicit_bounded_and_tenant_bound"),
    ("an import duplicates present cases",
     [(LIBRARY, "            .filter(|c| !self.cases.iter().any(|e| e.id == c.id))", "            .filter(|_| true)")],
     LO_T, "exports_and_imports_are_explicit_bounded_and_tenant_bound"),
    ("catalog examples are imported twice",
     [(LIBRARY, "                .any(|c| c.product == e.product && c.name.eq_ignore_ascii_case(&e.name))", "                .any(|_| false)")],
     LO_T, "catalogs_import_examples_once_and_report_unusable_catalogs"),
    ("a catalog example may have no goal",
     [(LIBRARY, "        if e.expected.trim().is_empty()\n            || ", "        if ")],
     LIB_T, "the_bundled_catalog_has_v1s_eleven_examples_across_five_products"),
    ("a catalog URL may name another host",
     [(LIBRARY, "        && url.host_str() == origin.host_str()\n", "")],
     LIB_T, "catalog_urls_follow_v1s_rule_for_the_configured_origin"),
    ("the library is written in plaintext",
     [(LIBRARY, "        crate::sessions::seal(&key, &aad(tenant), &plain).ok_or(LibraryFailure::Unavailable)?;", "        { let _ = key; plain.clone() };")],
     LO_T, "cases_are_authored_sealed_and_survive_a_restart"),
    ("an unreadable library is deleted",
     [(LIBRARY, '            let _ = std::fs::rename(&path, path.with_extension("unreadable"));', "            let _ = std::fs::remove_file(&path);")],
     LO_T, "a_locked_keychain_or_unreadable_library_is_reported_and_kept"),
    ("reset leaves case libraries",
     [(AUTHORITY, "                        if let Some(root) = &worker.adapters.library_root {", "                        if let Some(root) = None::<&std::path::PathBuf> {")],
     LO_T, "a_deleted_tenant_or_reset_takes_its_sealed_library_and_key"),
    ("a copy keeps the source identifier",
     [(LDOMAIN, "        copy.id = crate::library::new_id().ok_or(Refusal::Busy)?;\n", "")],
     LO_T, "a_case_copied_to_another_tenant_is_a_new_case_there"),
    ("benign is fabricated from inconsistent totals",
     [(OBS, "            let benign = s.checked_sub(v);", "            let benign = Some(s.saturating_sub(v));")],
     OBS_T, "summaries_never_fabricate_benign_or_detector_counts"),
    ("an empty breakdown while violating counts as zero detections",
     [(OBS, "            Some(Value::Array(entries)) if entries.is_empty() && violating > 0 => None,\n", "")],
     OBS_T, "summaries_never_fabricate_benign_or_detector_counts"),
    ("a key thirty days out is not an item",
     [(OBS, "        if days > EXPIRY_DAYS {", "        if days >= EXPIRY_DAYS {")],
     OBS_T, "key_expiry_is_inclusive_at_thirty_days_and_skips_inactive_keys"),
    ("revoked keys raise expiry items",
     [(OBS, '        if key.get("revoked").and_then(Value::as_bool) == Some(true)\n            || ', "        if ")],
     OBS_T, "key_expiry_is_inclusive_at_thirty_days_and_skips_inactive_keys"),
    ("Inbox items lose v1's order",
     [(OBS, "        (b.tone == Tone::Bad, b.action, &a.id).cmp(&(a.tone == Tone::Bad, a.action, &b.id))", "        a.id.cmp(&b.id)")],
     OBS_T, "key_expiry_is_inclusive_at_thirty_days_and_skips_inactive_keys"),
    ("an acknowledgement hides a worsened item",
     [(ODOMAIN, "                        HoldKind::Acknowledge => level(item) <= h.level,", "                        HoldKind::Acknowledge => true,")],
     LO_T, "a_failed_refresh_keeps_the_last_read_and_holds_follow_the_evidence"),
    ("a failed refresh replaces the last read",
     [(ODOMAIN, "                if record.read.is_none() {\n                    record.read = Some(overview);\n                }", "                record.read = Some(overview);")],
     LO_T, "a_failed_refresh_keeps_the_last_read_and_holds_follow_the_evidence"),
    ("a Runtime case may carry a payload",
     [(LIBRARY, "            || (self.product.can_scan() && !self.payload.is_null())\n", "")],
     LIB_T, "exports_and_imports_keep_v1s_file_and_refuse_foreign_or_malformed_files"),
    ("inconsistent totals get a share",
     [(OBS, "        per_mille: (sessions > 0 && violating <= sessions)", "        per_mille: (sessions > 0)")],
     OBS_T, "activity_needs_known_totals"),
    ("a deleted tenant's library key is not ledgered",
     [(AUTHORITY, "                state.data.pending_cleanup.push(library_key);\n", "")],
     LO_T, "a_failed_library_cleanup_stays_ledgered_and_finishes_later"),
    ("deadlines are never published",
     [(AUTHORITY, "                    if state.overview_published != now {", "                    if false {")],
     LO_T, "freshness_and_snooze_deadlines_are_published_without_any_event"),
    ("a failed write reports success",
     [(LDOMAIN, "                Err(_) => Err(CaseProblem::NotSaved),", "                Err(_) => Ok(notice),")],
     LO_T, "the_library_works_offline_survives_restart_and_retries_a_failed_save"),
    ("a specification offers Load",
     [(LVIEW, "                                    {c.product === 'runtime-security' ? (", "                                    {true ? (")],
     FE_LIBRARY, "loads a Runtime case into the scan form without scanning and only inspects specifications"),
    ("the overview reads again right after a failure",
     [(OVIEW, "        if (ready && !overview.fresh && !overview.refreshing && !recentlyFailed) void refresh()", "        if (ready && !overview.fresh && !overview.refreshing) void refresh()")],
     FE_OVERVIEW, "does not read again right after a failure and keeps the last read visible"),
    ("the overview reads although a fresh read is held",
     [(OVIEW, "        if (ready && !overview.fresh && !overview.refreshing && !recentlyFailed) void refresh()", "        if (ready && !overview.refreshing && !recentlyFailed) void refresh()")],
     FE_OVERVIEW, "reads on opening only when nothing fresh is held"),
    ("all clear is accepted without a fresh read",
     [(CLIENT, "        (value.face !== 'allClear' || (value.fresh === true && value.attention.length === 0))", "        true")],
     FE_SNAPSHOT, "rejects all clear without a fresh read"),
    ("a Runtime case may carry a payload",
     [(CLIENT, "        (value.product !== 'runtime-security' || value.payload === null)", "        true")],
     FE_SNAPSHOT, "rejects a Runtime case with a payload"),
]


def run(cmd):
    try:
        p = subprocess.run(cmd, cwd=REPO, env=ENV, capture_output=True, text=True, timeout=900)
    except subprocess.TimeoutExpired:
        return "timeout", ""
    return p.returncode, p.stdout + p.stderr


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


results = []
# An optional third argument runs only the probes whose name contains it.
ONLY = sys.argv[3] if len(sys.argv) > 3 else None
for name, edits, cmd, test in PROBES:
    if ONLY and ONLY not in name:
        continue
    saved = apply(edits)
    try:
        code, out = run(cmd)
    finally:
        restore(saved)
    results.append({"mutation": name, "files": sorted({e[0] for e in edits}), "command": cmd, "guard": test,
                    "exit_code": code, "guard_failed": code != 0 and test in out})
    print(name, code, results[-1]["guard_failed"], flush=True)
for cmd in (() if ONLY else BASELINES):
    code, _ = run(cmd)
    results.append({"baseline": cmd, "exit_code": code, "passes": code == 0})
    print("baseline", cmd[-1], code, flush=True)
with open(sys.argv[2], "w") as f:
    json.dump(results, f, indent=2)
