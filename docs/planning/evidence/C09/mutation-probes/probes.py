"""Deliberate behavior mutations for C09's reviewed Runtime changes and
one-time secrets: fresh-input and typed-confirmation binding, the native
confirmation's single-use completion and dismissal, conflict on a changed
baseline, v1 validation and authorship, auth-code injection, duplicate-name
checks, secret removal from views, the host registry, clipboard ownership,
the presentation's fresh capture and held editor, and contract validation.

Run in a disposable copy of the repository (never the reviewed checkout):
    python3 -I -B probes.py <copy-root> <results.json> [name filter]
Each probe applies one mutation (one or more exact, asserted replacements)
that violates a guarded invariant, runs the guarding target, requires it to
fail on the named test, and restores the files. Unmutated baselines of every
target must pass.
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
CHANGES_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "runtime_changes"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "changes"]
CONFIRM_T = ["cargo", "test", "--locked", "-p", "desktop-host", "--features", "test-fixtures", "--test", "native_confirmation"]
CLIP_T = ["cargo", "test", "--locked", "-p", "desktop-host", "--features", "test-fixtures", "--test", "clipboard_secret"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_APPLY = vitest("src/features/runtime/review-apply.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
BASELINES = (CHANGES_T, UNIT_T, CONFIRM_T, CLIP_T, FE_APPLY, FE_SNAPSHOT)

DOMAIN = "crates/desktop-core/src/authority/change_domain.rs"
CHANGES = "crates/desktop-core/src/changes.rs"
REGISTRY = "apps/desktop/src-tauri/src/native_confirmation.rs"
CLIPBOARD = "apps/desktop/src-tauri/src/clipboard_secret.rs"
WORKBENCH = "apps/desktop/src/features/administration/runtime-changes.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"

PROBES = [
    ("Apply ignores input edited after Review",
     [(DOMAIN, "        if fresh != current.draft {", "        if false {")],
     CHANGES_T, "fresh_input_and_the_typed_confirmation_must_match_the_review"),
    ("Apply ignores the typed confirmation",
     [(DOMAIN, "        if expected.as_deref() != Some(typed) {", "        if false {")],
     CHANGES_T, "fresh_input_and_the_typed_confirmation_must_match_the_review"),
    ("a declined confirmation still applies",
     [(DOMAIN, "        if !accepted {\n            current.phase = Phase::Ready(prepared);", "        if false {\n            current.phase = Phase::Ready(prepared);")],
     CHANGES_T, "a_denied_or_stale_confirmation_sends_nothing"),
    ("retiring a review leaves its sheet open",
     [(DOMAIN, "            if active.as_ref() != Some(&review.context) {\n                if let Phase::Confirming { nonce, .. } = review.phase {\n                    self.confirmer.dismiss(nonce);\n                }",
       "            if active.as_ref() != Some(&review.context) {\n                if let Phase::Confirming { nonce: _, .. } = review.phase {\n                }")],
     CHANGES_T, "a_denied_or_stale_confirmation_sends_nothing"),
    ("a changed baseline is written over",
     [(DOMAIN, "        if digest != self.prepared.digest {", "        if false {")],
     CHANGES_T, "a_resource_changed_since_review_is_a_conflict_and_is_not_written"),
    ("a duplicate name is accepted for a new key",
     [(DOMAIN, "        Walk::Found(_) => Err(exists.into()),", "        Walk::Found(_) => Ok(()),")],
     CHANGES_T, "key_creation_checks_names_injects_the_deployment_code_and_holds_the_secret"),
    ("write responses keep their secret fields",
     [(CHANGES, "    redact(&mut response);\n    (response, secret)", "    (response, secret)")],
     CHANGES_T, "key_creation_checks_names_injects_the_deployment_code_and_holds_the_secret"),
    ("a draft may carry its own auth code",
     [(CHANGES, "    body.remove(\"auth_code\");\n", "")],
     UNIT_T, "key_creation_needs_its_fields_and_one_deployment_profiles_code"),
    ("topics keep the draft's author",
     [(CHANGES, "            body.insert(field.into(), author);\n            body.remove(\"last_modified_ts\");", "            let _ = (field, author);\n            body.remove(\"last_modified_ts\");")],
     UNIT_T, "topics_are_authored_stripped_and_bounded"),
    ("a rotation sends the whole draft",
     [(CHANGES, "                body = reduced;", "                let _ = reduced;")],
     UNIT_T, "key_rotation_is_reduced_and_bounded"),
    ("an ambiguous deployment profile supplies a code",
     [(CHANGES, "    let [only] = matches.as_slice() else {", "    let Some(only) = matches.first() else {")],
     UNIT_T, "key_creation_needs_its_fields_and_one_deployment_profiles_code"),
    ("a confirmation nonce can be opened twice",
     [(REGISTRY, "        if pending.contains_key(&nonce) {\n            return None;\n        }\n", "")],
     CONFIRM_T, "a_confirmation_resolves_once_and_only_for_its_nonce"),
    ("cleanup clears a clipboard someone else wrote",
     [(CLIPBOARD, "        let ours = self.board.change_count() == copied.change_count", "        let ours = true || self.board.change_count() == copied.change_count")],
     CLIP_T, "only_an_unchanged_own_copy_is_cleared"),
    ("Review and Apply send the editor's initial text",
     [(WORKBENCH, "body: deletes ? '{}' : editor.current?.value ?? ''", "body: deletes ? '{}' : change.body")],
     FE_APPLY, "reviews exactly the editor text and applies the text present when Apply is pressed"),
    ("the editor stays writable while the confirmation is open",
     [(WORKBENCH, "                        readOnly={confirming}\n                        spellCheck={false}\n                        aria-describedby", "                        readOnly={false}\n                        spellCheck={false}\n                        aria-describedby")],
     FE_APPLY, "holds the editor while the native confirmation is open"),
    ("an uncertain outcome is shown as applied",
     [(WORKBENCH, "    if (result.failure) {", "    if (result.failure && result.outcome !== 'unknown') {")],
     FE_APPLY, "reports an uncertain outcome without claiming success"),
    ("the presentation accepts an unsettled change with an outcome",
     [(CLIENT, "(value.settled ? oneOf(OUTCOMES, value.outcome) : value.outcome === null)", "true")],
     FE_SNAPSHOT, "rejects an unsettled change with an outcome"),
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
