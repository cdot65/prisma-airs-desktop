"""Deliberate behavior mutations for C10's DLP authoring subset: v1's
allowed matrix, data profile structure rules and legacy-field removal,
dictionary bounds and file names, staged keyword files (token binding,
size, byte-order mark), DLP readback and the 204 dictionary replace, the
presentation's file reference and per-inventory actions, and contract
validation.

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
DLP_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "dlp_subset"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "changes"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_DLP = vitest("src/features/administration/dlp-authoring.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (DLP_T, UNIT_T, FE_DLP, FE_SNAPSHOT)

CHANGES = "crates/desktop-core/src/changes.rs"
DOMAIN = "crates/desktop-core/src/authority/change_domain.rs"
MGMT = "crates/desktop-core/src/management.rs"
WORKBENCH = "apps/desktop/src/features/administration/runtime-changes.tsx"
ADMIN = "apps/desktop/src/features/administration/administration-view.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"

PROBES = [
    ("filtering profiles can be deleted",
     [(CHANGES, "            | (ChangeKind::FilteringProfiles, Update)", "            | (ChangeKind::FilteringProfiles, Update | Delete)")],
     DLP_T, "data_profiles_follow_v1s_structure_rules_and_have_no_delete"),
    ("data profiles can be deleted",
     [(CHANGES, "            | (ChangeKind::DataProfiles, Create | Update)", "            | (ChangeKind::DataProfiles, Create | Update | Delete)")],
     DLP_T, "data_profiles_follow_v1s_structure_rules_and_have_no_delete"),
    ("the legacy rendering is sent back with a data profile",
     [(CHANGES, "            body.remove(\"advance_data_patterns_rule_request\");\n", "")],
     DLP_T, "data_profiles_follow_v1s_structure_rules_and_have_no_delete"),
    ("data profiles skip v1's structure rules",
     [(CHANGES, "            if !valid_data_profile(&body) {", "            if false {")],
     DLP_T, "data_profiles_follow_v1s_structure_rules_and_have_no_delete"),
    ("a pattern leaf may omit its confidence level",
     [(CHANGES, "                || !choice(get(\"confidence_level\"), &CONFIDENCE)\n", "")],
     UNIT_T, "data_profiles_follow_v1s_structure_rules"),
    ("dictionary files are unbounded",
     [(CHANGES, "            if !text.is_empty() && text.encode_utf16().count() <= MAX_DICTIONARY_FILE =>", "            if !text.is_empty() =>")],
     DLP_T, "dictionary_files_and_names_are_bounded_before_any_request"),
    ("a dictionary file name may hold a path",
     [(CHANGES, "    if !name_ok {", "    if false {")],
     DLP_T, "dictionary_files_and_names_are_bounded_before_any_request"),
    ("any staged token resolves",
     [(DOMAIN, "            Some(Staging::Ready(file))\n                if &file.context == context && Some(file.token.as_str()) == reference.as_str() =>",
       "            Some(Staging::Ready(file)) if &file.context == context =>")],
     DLP_T, "a_file_chosen_in_the_native_sheet_is_staged_validated_and_uploaded"),
    ("an oversized chosen file is staged",
     [(DOMAIN, "    if text.encode_utf16().count() > crate::changes::MAX_DICTIONARY_FILE {", "    if false {")],
     DLP_T, "a_file_chosen_in_the_native_sheet_is_staged_validated_and_uploaded"),
    ("a chosen file's byte-order mark is uploaded",
     [(DOMAIN, ".strip_prefix(&[0xEF, 0xBB, 0xBF])", ".strip_prefix(&[0x00, 0x00, 0x00])")],
     DLP_T, "a_file_chosen_in_the_native_sheet_is_staged_validated_and_uploaded"),
    ("a deleted DLP item reads back as present",
     [(DOMAIN, "            Ok(None) => (Readback::Absent, None),\n            Err(_) => (Readback::Failed, None),\n        };\n    }",
       "            Ok(None) => (Readback::Found, None),\n            Err(_) => (Readback::Failed, None),\n        };\n    }")],
     DLP_T, "data_patterns_are_created_replaced_and_deleted_exactly"),
    ("a dictionary replace without a body is an invalid reply",
     [(MGMT, "                        None => Ok(Value::Object(serde_json::Map::new())),", "                        None => Err(PageError::Invalid),")],
     DLP_T, "a_typed_dictionary_is_uploaded_as_literal_multipart"),
    ("the presentation sends the token as file content",
     [(WORKBENCH, "? { metadata, fileRef: staged.token }", "? { metadata, file: staged.token }")],
     FE_DLP, "references a file chosen in the native sheet by its token only"),
    ("data profiles offer a delete",
     [(ADMIN, "        case 'dataProfiles':\n        case 'filteringProfiles':\n            return [edit]", "        case 'dataProfiles':\n        case 'filteringProfiles':\n            return [edit, remove]")],
     FE_DLP, "offers exactly v1’s actions per inventory"),
    ("the presentation accepts a path as a staged token",
     [(CLIENT, "typeof value.token === 'string' && STAGED_TOKEN.test(value.token)", "typeof value.token === 'string'")],
     FE_SNAPSHOT, "rejects a staged file whose token is a path"),
    ("a dictionary preview carries the file text",
     [(CHANGES, "        if mutation.kind == ChangeKind::Dictionaries\n", "        if false && mutation.kind == ChangeKind::Dictionaries\n")],
     DLP_T, "a_staged_files_content_never_reaches_a_projection_and_the_pending_hold_keeps_it"),
    ("a file can be chosen while a confirmation holds the editor",
     [(DOMAIN, "        if matches!(self.staging, Some(Staging::Choosing { .. }))\n            || matches!(", "        if matches!(self.staging, Some(Staging::Choosing { .. }))\n            || false && matches!(")],
     DLP_T, "a_staged_files_content_never_reaches_a_projection_and_the_pending_hold_keeps_it"),
    ("a cancelled file choice leaves the file option in use",
     [(WORKBENCH, "    const usingFile = useFile && fileReady", "    const usingFile = useFile"),
      (WORKBENCH, "        setUseFile(staged?.state === 'ready')", "        setUseFile(true)")],
     FE_DLP, "keeps typed keywords in use when a file choice is cancelled or fails"),
    ("raw metadata is spliced into the dictionary body",
     [(WORKBENCH, "            if (typeof metadata !== 'object' || metadata === null || Array.isArray(metadata)) {", "            if (false) {")],
     FE_DLP, "refuses metadata that is not exactly one JSON object without dispatching"),
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
