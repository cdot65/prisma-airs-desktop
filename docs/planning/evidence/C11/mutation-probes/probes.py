"""Deliberate behavior mutations for C11's sessions investigations and
metadata-only reports: content stripping and explicit reveal, v1's search
classification, bounds and coverage, the investigation cache (Keychain-only
sealing, plaintext-free snapshots, removal with the credential and on
reset), report construction, validation, upsert/undo and CSV escaping,
tenant deletion, metadata-only exports, the presentation's reveal and
coverage wording, and contract validation.

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
SR_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "sessions_reports"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "sessions"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_SESSIONS = vitest("src/features/sessions/sessions-view.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (SR_T, UNIT_T, FE_SESSIONS, FE_SNAPSHOT)

SESSIONS = "crates/desktop-core/src/sessions.rs"
MGMT = "crates/desktop-core/src/management.rs"
DOMAIN = "crates/desktop-core/src/authority/sessions_domain.rs"
MDOMAIN = "crates/desktop-core/src/authority/management_domain.rs"
AUTHORITY = "crates/desktop-core/src/authority.rs"
VIEW = "apps/desktop/src/features/sessions/sessions-view.tsx"
MODEL = "apps/desktop/src/features/sessions/sessions-model.ts"
CLIENT = "packages/desktop-contracts/src/client.ts"

PROBES = [
    ("a session read keeps its prompts",
     [(SESSIONS, '            for key in [\n                "prompt",\n', '            for key in [\n')],
     SR_T, "content_is_read_only_on_reveal_and_never_kept_with_metadata"),
    ("a transaction keeps its content",
     [(MGMT, "        crate::sessions::strip_content(&mut value);\n        Ok(value)", "        Ok(value)")],
     SR_T, "content_is_read_only_on_reveal_and_never_kept_with_metadata"),
    ("a revealed scan is read again instead of from the cache",
     [(MGMT, "                if let Some(content) = cached {\n                    return Ok(ManagementResult::Content { content });\n                }\n", "")],
     SR_T, "content_is_read_only_on_reveal_and_never_kept_with_metadata"),
    ("session IDs are not classified",
     [(SESSIONS, '    if lower.starts_with("ses") || lower.starts_with("session") {', "    if false {")],
     SR_T, "searches_are_bounded_like_v1_and_say_what_they_covered"),
    ("a detector matches any violating entry",
     [(SESSIONS, '                e.get("detection_type").and_then(Value::as_str) == Some(detector)\n                    && ', "                ")],
     SR_T, "searches_are_bounded_like_v1_and_say_what_they_covered"),
    ("the session ID walk reads past 40 pages",
     [(MGMT, "                        SEARCH_PAGE * SESSION_SEARCH_PAGES,", "                        SEARCH_PAGE * SESSION_SEARCH_PAGES * 2,")],
     SR_T, "searches_are_bounded_like_v1_and_say_what_they_covered"),
    ("the detector search ignores its match bound",
     [(MGMT, "                let limit = DETECTOR_MATCHES as usize;", "                let limit = usize::MAX;")],
     SR_T, "searches_are_bounded_like_v1_and_say_what_they_covered"),
    ("a capped session ID walk claims absence",
     [(MGMT, "                search.complete = complete;\n                search.scanned = scanned;\n                search.sessions = Value::Array(found).to_string();\n            }\n            SearchKind::ScanId",
       "                search.complete = true;\n                search.scanned = scanned;\n                search.sessions = Value::Array(found).to_string();\n            }\n            SearchKind::ScanId")],
     SR_T, "searches_are_bounded_like_v1_and_say_what_they_covered"),
    ("a scan ID search returns content",
     [(MGMT, "                        crate::sessions::strip_content(&mut report);\n                        search.scan", "                        search.scan")],
     SR_T, "searches_are_bounded_like_v1_and_say_what_they_covered"),
    ("a session credential's investigation is sealed on disk",
     [(MDOMAIN, "            (mode == StorageMode::Keychain)", "            (mode == mode)")],
     SR_T, "a_session_credentials_investigation_stays_in_memory"),
    ("the snapshot is written in plaintext",
     [(SESSIONS, "            .and_then(|key| seal(&key, &self.aad(), &bytes))", "            .map(|_key| bytes.clone())")],
     SR_T, "a_keychain_credentials_investigation_is_sealed_and_survives_a_restart"),
    ("a removed credential leaves its sealed investigation",
     [(AUTHORITY, "                    investigation\n                        && matches!(", "                    matches!(")],
     SR_T, "removing_the_credential_removes_its_sealed_investigation_and_key"),
    ("reset leaves sealed investigations and reports",
     [(AUTHORITY, "                        if let Some(root) = &worker.adapters.sessions_root {", "                        if let Some(root) = None::<&std::path::PathBuf> {")],
     SR_T, "reset_removes_sealed_investigations_and_reports"),
    ("an edit resets the report time",
     [(SESSIONS, "                edited.reported_at = previous.reported_at.clone();\n", "")],
     SR_T, "reports_are_built_from_scan_metadata_and_kept_edited_removed_and_undone"),
    ("any category is accepted",
     [(SESSIONS, "    if !report.category.is_empty() && !report.kind.categories()", "    if false && !report.kind.categories()")],
     SR_T, "reports_are_built_from_scan_metadata_and_kept_edited_removed_and_undone"),
    ("a report is built from another scan of the session",
     [(DOMAIN, "                                row.get(\"scan_id\").and_then(serde_json::Value::as_str)\n                                    == Some(target.scan_id.as_str())\n                                    && ",
       "                                ")],
     SR_T, "reports_are_built_from_scan_metadata_and_kept_edited_removed_and_undone"),
    ("a deleted tenant's reports stay on disk",
     [(AUTHORITY, "            if let Some(path) = state.forget_reports(&tenant) {\n                effects.push(Effect::ForgetReports(path));\n            }",
       "            let _ = state.forget_reports(&tenant);")],
     SR_T, "reports_are_built_from_scan_metadata_and_kept_edited_removed_and_undone"),
    ("undo overwrites a newer report",
     [(SESSIONS, "            .is_some()\n            || self.reports.len() >= MAX_REPORTS", "            .is_none()\n            || self.reports.len() >= MAX_REPORTS")],
     UNIT_T, "reports_upsert_remove_and_undo_without_overwriting_newer"),
    ("CSV cells lose v1's formula guard",
     [(SESSIONS, "    let guarded = if !numeric && value.starts_with(['=', '+', '-', '@', '\\t', '\\r']) {", "    let guarded = if false {")],
     SR_T, "reports_export_as_v1_csv_through_the_save_sheet"),
    ("the metadata export carries revealed content",
     [(DOMAIN, '            "transactions": metadata["transactions"],',
       '            "transactions": metadata["transactions"],\n            "contents": investigation.with(|c| c.contents.iter().map(|(_, v)| v.clone()).collect::<Vec<_>>()),')],
     SR_T, "clearing_empties_the_cache_and_exports_carry_metadata_only"),
    ("Hide keeps the content shown",
     [(VIEW, "            const { [scanKey(scan)]: _, ...rest } = r\n            return rest", "            return r")],
     FE_SESSIONS, "shows content only after an explicit reveal and forgets it on Hide and on leaving"),
    ("reports open without a management client",
     [(VIEW, "        if (context && ready) void dispatch({ kind: 'openReports', context })", "        if (context) void dispatch({ kind: 'openReports', context })")],
     FE_SESSIONS, "asks for a management client before reading sessions"),
    ("a capped session ID search claims absence",
     [(MODEL, "            return search.complete\n                ? `No session with this ID", "            return true\n                ? `No session with this ID")],
     FE_SESSIONS, "says what each search covered and never claims more"),
    ("a cancelled export may carry a file name",
     [(CLIENT, "(e.state === 'saved' ? isString(e.name) : e.name === null)", "true")],
     FE_SNAPSHOT, "rejects a cancelled export with a name"),
    ("a report of an unknown kind is accepted",
     [(CLIENT, "        oneOf(REPORT_KINDS, value.kind) &&", "        isString(value.kind) &&")],
     FE_SNAPSHOT, "rejects a report with an unknown kind"),
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
