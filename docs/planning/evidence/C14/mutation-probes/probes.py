"""Deliberate behavior mutations for C14's API Client projects and
documents: fresh-snapshot Save with revisions, faithful writes (rows,
settings, comments, line endings), conflicts and the save journal, sealed
drafts, paths inside the collection, duplicate identities, masked previews
and environment precedence, tenant references, cURL and Insomnia import
policy, bounded YAML, the hardened Git view, the built-in collection's
refresh, reset and the close guard; and the presentation and contract rules
(fresh capture, replies never overwriting newer typing, protection on
leaving, the menu Save, relative paths, problem details).

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
WB_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "workbench_documents"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "workbench"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_SAVE = vitest("src/features/workbench/document-save.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (WB_T, UNIT_T, FE_SAVE, FE_SNAPSHOT)

DOC = "crates/desktop-core/src/workbench/document.rs"
PROJECT = "crates/desktop-core/src/workbench/project.rs"
ENVS = "crates/desktop-core/src/workbench/environments.rs"
PROFILE = "crates/desktop-core/src/workbench/profile.rs"
CURL = "crates/desktop-core/src/workbench/curl.rs"
INSOMNIA = "crates/desktop-core/src/workbench/insomnia.rs"
GIT = "crates/desktop-core/src/workbench/git.rs"
SCM = "crates/desktop-core/src/workbench/scm.rs"
DOMAIN = "crates/desktop-core/src/authority/workbench_domain.rs"
AUTHORITY = "crates/desktop-core/src/authority.rs"
EDITOR = "apps/desktop/src/features/workbench/document-editor.tsx"
VIEW = "apps/desktop/src/features/workbench/workbench-view.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"

STALE = "stale_and_invalid_snapshots_never_write_and_typing_during_save_stays_protected"
CONFLICT = "external_changes_become_conflicts_and_both_versions_are_archived"
INTERRUPTED = "an_interrupted_save_is_found_after_a_restart_and_nothing_replays"
SECRETS = "environments_and_workbench_secrets_resolve_masked_in_the_preview"

PROBES = [
    ("a stale revision is accepted",
     [(DOMAIN, "    if doc.revision() != revision {\n        return Err(WorkbenchProblem::Stale);\n    }\n", "")],
     WB_T, STALE),
    ("invalid header rows are written",
     [(DOC, "    header_rows(&fields.headers)?;\n", "")],
     WB_T, STALE),
    ("inline credentials are accepted in settings",
     [(DOC, ".all(|(k, v)| AUTH_KEYS.contains(&k) && v.is_str())", ".all(|(_, v)| v.is_str())")],
     UNIT_T, "settings_hold_references_never_credentials"),
    ("files changed on disk are overwritten",
     [(DOC, "        Ok(false) => {\n            return Err(if document.new {\n                SaveRefusal::Exists\n            } else {\n                SaveRefusal::Conflict\n            });\n        }\n",
       "        Ok(false) => {}\n")],
     WB_T, CONFLICT),
    ("comments on edited values are dropped",
     [(DOC, "        *replacement.decor_mut() = old.decor().clone();\n", "")],
     UNIT_T, "edits_keep_comments_extensions_and_line_endings"),
    ("CRLF files are rewritten with LF",
     [(DOC, "        Some(if self.syntax.contains(\"\\r\\n\") {", "        Some(if false {")],
     UNIT_T, "edits_keep_comments_extensions_and_line_endings"),
    ("files are written before the journal",
     [(DOMAIN, "    profile::store_journal(private, keychain, scope, &journal).map_err(|_| SaveRefusal::Io)?;\n", "")],
     WB_T, INTERRUPTED),
    ("an interrupted save looks unstarted",
     [(PROFILE, "        _ => JournalState::Interrupted,", "        _ => JournalState::NotStarted,")],
     WB_T, INTERRUPTED),
    ("private data is stored unsealed",
     [(PROFILE, "        crate::sessions::seal(&key, &aad(name), &plain).ok_or(PrivateFailure::Unavailable)?;", "        Some(plain.clone()).ok_or(PrivateFailure::Unavailable)?;")],
     UNIT_T, "sealed_files_open_only_under_their_key_and_name"),
    ("writes follow symbolic links",
     [(PROJECT, "            Ok(meta) if meta.file_type().is_symlink() => return Err(ProjectProblem::BadPath),\n", "")],
     UNIT_T, "writes_refuse_links_git_and_nested_collections"),
    ("parent components are accepted in paths",
     [(PROJECT, "            || part == \"..\"\n", ""),
      (PROJECT, "    if path.components().all(|c| matches!(c, Component::Normal(_))) {", "    if true {")],
     UNIT_T, "paths_stay_inside_and_visible"),
    ("duplicate identities stay in the inventory",
     [(PROJECT, "            catalog.requests.retain(|r| r.id != id);\n", "")],
     UNIT_T, "discovery_reports_duplicates_links_and_invalid_files"),
    ("a secret variable shows its reference",
     [(ENVS, "            Some((Value::Secret(_), _)) => {\n                used.push(name.to_owned());\n                out.push_str(MASK);",
       "            Some((Value::Secret(r), _)) => {\n                used.push(name.to_owned());\n                out.push_str(r);")],
     UNIT_T, "layers_apply_in_order_and_secrets_stay_masked"),
    ("shared environments override private ones",
     [(ENVS, "        (&layers.shared, Layer::Shared),\n        (&layers.local, Layer::Local),", "        (&layers.local, Layer::Local),\n        (&layers.shared, Layer::Shared),")],
     UNIT_T, "layers_apply_in_order_and_secrets_stay_masked"),
    ("tenant references are stored as workbench secrets",
     [(DOMAIN, "    if reference.starts_with(scm::MANAGED_PREFIX) {\n        return refused(WorkbenchProblem::Managed);\n    }\n", "")],
     WB_T, SECRETS),
    ("cURL credential headers stay literal",
     [(CURL, "        if credential_name(name) {", "        if false {")],
     UNIT_T, "converts_a_literal_command_and_removes_credentials"),
    ("shell operators are taken literally",
     [(CURL, "                | ']' | '~' | '{' | '}' => return Err(CurlProblem::Shell),", "                | ']' | '~' | '{' | '}' => {\n                    word.push(c);\n                    started = true;\n                }")],
     UNIT_T, "refuses_what_it_cannot_import_faithfully"),
    ("Insomnia scripts are converted",
     [(INSOMNIA, "    if !keys(source, &REQUEST_KEYS) {", "    if false {")],
     UNIT_T, "converts_supported_requests_and_refuses_scripts"),
    ("YAML expansion is unbounded",
     [(INSOMNIA, "        if self.depth >= MAX_DEPTH || self.nodes.get() >= MAX_NODES {", "        if false {")],
     UNIT_T, "refuses_other_formats_and_oversized_structures"),
    ("Git runs a repository fsmonitor",
     [(GIT, "            \"-c\",\n            \"core.fsmonitor=false\",\n", "")],
     UNIT_T, "repository_hooks_and_fsmonitor_never_run"),
    ("a refresh overwrites edited built-in files",
     [(SCM, "            if recorded == Some(&current) || existing == bytes {", "            if true {")],
     UNIT_T, "refresh_keeps_edits_and_respects_deletions"),
    ("reset keeps the private folder",
     [(AUTHORITY, "                        if let Some(root) = &worker.adapters.workbench_root {\n                            match std::fs::remove_dir_all(root) {",
       "                        if let Some(root) = None::<&std::path::PathBuf> {\n                            match std::fs::remove_dir_all(root) {")],
     WB_T, "reset_removes_the_private_folder_and_keeps_the_operators_collections"),
    ("the close guard ignores unprotected edits",
     [(DOMAIN, "        !self.workbench.editing.is_empty()\n", "        false\n")],
     WB_T, "requests_wait_their_turn_and_the_close_guard_sees_unprotected_edits"),
    ("Save sends the loaded fields",
     [(EDITOR, "        const snapshot = fieldsRef.current\n", "        const snapshot = doc.fields\n")],
     FE_SAVE, "sends the fields typed before the protection debounce fires"),
    ("a save reply overwrites newer typing",
     [(EDITOR, "            ackGen.current = Math.max(ackGen.current, edit)\n            adopt(r.document, null)", "            ackGen.current = Math.max(ackGen.current, edit)\n            adopt(r.document, r.document.fields)")],
     FE_SAVE, "keeps text typed while a save is in flight and protects it against the new revision"),
    ("a refused save clears the editor",
     [(EDITOR, "        if (r.kind === 'saveRefused') {\n", "        if (r.kind === 'saveRefused') {\n            fieldsRef.current = doc.saved\n            setFields(doc.saved)\n")],
     FE_SAVE, "refuses invalid input without changing what the editor holds"),
    ("leaving a document drops pending edits",
     [(EDITOR, "            const left = flush()\n", "            const left = Promise.resolve(true)\n")],
     FE_SAVE, "protects pending edits when leaving the document"),
    ("every keystroke reports editing",
     [(EDITOR, "        if (markedGen.current <= sentGen.current) {", "        if (true) {")],
     FE_SAVE, "protects the snapshot when typing pauses, marking unprotected edits once"),
    ("the menu Save is ignored",
     [(EDITOR, "        window.addEventListener(SAVE_DOCUMENT_EVENT, onSave)\n", "")],
     FE_SAVE, "sends the fields typed before the protection debounce fires"),
    ("parent paths pass the contract",
     [(CLIENT, "part !== '' && part !== '..' && part !== '.'", "part !== '' && part !== '.'")],
     FE_SNAPSHOT, "rejects paths, identities and revisions outside what Rust issues"),
    ("problem details are not checked",
     [(CLIENT, "    return details === null ? value.detail === undefined : oneOf(details, value.detail)", "    return true")],
     FE_SNAPSHOT, "rejects paths, identities and revisions outside what Rust issues"),
    # Attempt-01 remediation (R1-R5).
    ("an older acknowledgement clears newer edits",
     [(DOMAIN, "        && state.workbench.editing.get(id).is_some_and(|m| *m <= edit)", "        && state.workbench.editing.get(id).is_some()")],
     WB_T, "an_older_acknowledgement_never_clears_newer_unprotected_edits"),
    ("editing keeps only its first generation",
     [(DOMAIN, "                *mark = (*mark).max(edit);\n", "")],
     WB_T, "an_older_acknowledgement_never_clears_newer_unprotected_edits"),
    ("closing a tab drops unprotected edits",
     [(DOMAIN, "            if !discard\n                && lock(inner)", "            if false\n                && lock(inner)")],
     WB_T, "closing_with_unprotected_edits_waits_for_protection_or_an_explicit_discard"),
    ("Save writes before protecting its snapshot",
     [(DOMAIN, "    if let Err(f) = persist(inner, root).await {\n        return refused(WorkbenchProblem::Private(f));\n    }\n    let interrupt", "    let interrupt")],
     WB_T, "a_save_stopped_between_writes_restores_every_submitted_field"),
    ("documents open from the inventory",
     [(DOC, "        method,\n        url,\n        headers: rows_from", "        method: entry.method.clone(),\n        url: entry.url.clone(),\n        headers: rows_from")],
     WB_T, "documents_open_from_their_own_metadata_not_the_inventory"),
    ("duplicate requests lose their body ownership",
     [(DOC, "        .is_some_and(|owners| owners.iter().any(|r| r != relative))", "        .is_some_and(|_| false)")],
     WB_T, "a_duplicate_identity_pair_still_owns_its_body"),
    ("an outstanding protection silences new edits",
     [(EDITOR, "        sentGen.current = Math.max(sentGen.current, edit)\n        return call({ kind: 'protect'", "        return call({ kind: 'protect'")],
     FE_SAVE, "re-arms the close guard after a save and while an older protection is outstanding"),
    ("closing does not protect first",
     [(VIEW, "            const kept = await Promise.all(handles.map((h) => (h ? h.flush() : Promise.resolve(true))))", "            const kept = [true]")],
     FE_SAVE, "protects the latest typing before a tab closes"),
    ("closing never asks when protection fails",
     [(VIEW, "            if (kept.includes(false)) {", "            if (false) {")],
     FE_SAVE, "asks before closing the collection when the latest typing cannot be kept"),
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
