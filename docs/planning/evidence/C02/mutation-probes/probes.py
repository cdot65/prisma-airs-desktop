"""Deliberate behavior mutations for C02's security-critical guards.

Run in a disposable copy of the repository (never the reviewed checkout):
    python3 -I -B probes.py <copy-root> <results.json>
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
CORE = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "context_credentials"]
CORE_LIB = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib"]
KEYCHAIN = ["cargo", "test", "--locked", "-p", "desktop-host", "--features", "test-fixtures", "--test", "keychain_access"]
FE = ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts",
      "src/features/tenants/credential-entry.test.tsx", "src/features/tenants/profile-import.test.tsx",
      "src/features/tenants/reset.test.tsx", "src/contracts/snapshot-validation.test.ts"]

AUTH = "crates/desktop-core/src/authority.rs"
TRANSPORT = "crates/desktop-core/src/transport.rs"
PROBES = [
    ("a stale write completion attaches its record",
     [(AUTH, "    let current = state.tenant(tenant).is_some_and(", "    let current = true || state.tenant(tenant).is_some_and(")],
     CORE, "a_switch_during_a_write_leaves_the_stale_record_unattached_and_cleaned"),
    ("cleanup deletes a record whose write is in flight",
     [(AUTH, "        let in_flight = state.writes_in_flight.clone();", "        let in_flight: Vec<RecordId> = Vec::new();")],
     CORE, "a_switch_during_a_write_leaves_the_stale_record_unattached_and_cleaned"),
    ("concurrent writes for one tenant admitted",
     [(AUTH, "            if matches!(current.scan, ScanCapability::Storing { .. }) {\n                return Err(Refusal::Busy);",
       "            if false {\n                return Err(Refusal::Busy);")],
     CORE, "a_switch_during_a_write_leaves_the_stale_record_unattached_and_cleaned"),
    ("any context reference accepted",
     [(AUTH, "(Some(current), Some(tenant)) if current == context => Ok(tenant.clone()),",
       "(Some(_current), Some(tenant)) if !context.as_str().is_empty() => Ok(tenant.clone()),")],
     CORE, "tenants_are_created_selected_and_contexts_never_revive"),
    ("endpoint change keeps the old key",
     [(AUTH, "            let retired_session = state.retire_credential(&tenant);\n            if let Some(t) = state.tenant_mut(&tenant) {\n                t.profile.destination = to;",
       "            let retired_session: Option<RecordId> = None;\n            if let Some(t) = state.tenant_mut(&tenant) {\n                t.profile.destination = to;")],
     CORE, "endpoint_changes_are_reviewed_and_never_carry_the_old_key"),
    ("session-only keys persisted",
     [(AUTH, "                    ScanCapability::Ready {\n                        record,\n                        mode: StorageMode::Keychain,\n                        destination,\n                    } => Some(StoredCredential {",
       "                    ScanCapability::Ready {\n                        record,\n                        mode: _,\n                        destination,\n                    } => Some(StoredCredential {")],
     CORE, "session_only_keys_are_never_persisted_and_end_with_the_app"),
    ("reset purges while a credential write is in flight",
     [(AUTH, "                        if pending == 0 {", "                        if pending == 0 || pending > 0 {")],
     CORE, "reset_waits_for_in_flight_writes_and_retires_authority_at_admission"),
    ("reset leaves the prior context usable",
     [(AUTH, "        if self.storage == StorageHealth::Resetting {\n            return Err(Refusal::StaleContext);\n        }\n", ""),
      (AUTH, "        self.active_ref = None;\n", "")],
     CORE, "reset_waits_for_in_flight_writes_and_retires_authority_at_admission"),
    ("reset keeps credential bindings until it succeeds",
     [(AUTH, "                .filter_map(|id| state.retire_credential(id))", "                .filter_map(|_id| None::<RecordId>)")],
     CORE, "a_failed_purge_retires_keys_durably_and_cleanup_finishes_after_restart"),
    ("a key is written before its write-ahead entry is confirmed durable",
     [(AUTH, "            let authorized = mode == StorageMode::Session\n                || !state.durability_unconfirmed\n                || state.confirm_durable();",
       "            let authorized = true;")],
     CORE, "a_first_key_is_written_only_after_a_confirmed_write_ahead"),
    ("an unconfirmed (written) save is rolled back and refused",
     [(AUTH, "            Err(StoreError::Unconfirmed) => {\n                self.durability_unconfirmed = true;\n                Ok(())\n            }",
       "            Err(StoreError::Unconfirmed) => {\n                self.data = before.clone();\n                Err(Refusal::DeniedStore)\n            }")],
     CORE, "saves_are_refused_only_when_nothing_was_written"),
    ("snapshot validator accepts missing import counts",
     [("packages/desktop-contracts/src/client.ts",
       "                isCount(value.discarded.secretFields) &&\n                isCount(value.discarded.fileReferences) &&\n                isCount(value.discarded.otherFields) &&\n", "")],
     FE, "rejects discarded counts missing"),
    ("redirects followed",
     [(TRANSPORT, ".redirect(reqwest::redirect::Policy::none())", ".redirect(reqwest::redirect::Policy::limited(3))")],
     CORE, "redirects_are_returned_to_the_sdk_not_followed"),
    ("Direct adopts the ambient proxy",
     [(TRANSPORT, "ProxyMode::Direct => builder.no_proxy(),", "ProxyMode::Direct => builder,")],
     CORE, "proxy_modes_follow_the_policy_not_ambient_settings"),
    ("unparseable extra roots silently ignored",
     [(TRANSPORT, "    if certificates.is_empty() {\n        return Err(PolicyError::Root);\n    }\n", ""),
      (TRANSPORT, "        webpki::anchor_from_trusted_cert(certificate).map_err(|_| PolicyError::Root)?;", "        let _ = certificate;")],
     CORE_LIB, "invalid_policies_are_refused"),
    ("import takes a secret field as the label",
     [("crates/desktop-core/src/import.rs", 'ImportFormat::AirsCliRegistry => ("name", &["name", "tsgId"]),',
       'ImportFormat::AirsCliRegistry => ("airsApiKey", &["name", "tsgId", "airsApiKey"]),')],
     CORE_LIB, "airs_cli_registry_imports_names_and_tsg_ids_only"),
    ("a locked keychain reported as denied",
     [("apps/desktop/src-tauri/src/keychain.rs", "            if locked == Some(true) =>", "            if false =>")],
     KEYCHAIN, "a_locked_keychain_reports_locked_and_recovers_after_unlock"),
    ("purge not limited to the v2 namespace",
     [("apps/desktop/src-tauri/src/keychain.rs", "                .service(self.service)\n", "")],
     KEYCHAIN, "purge_removes_only_the_v2_namespace"),
    ("entered key left in the field while awaiting",
     [("apps/desktop/src/features/tenants/credential-entry.tsx", "        field.value = ''\n", "")],
     FE, "captures the key typed at submission time and clears the field before awaiting"),
    ("invalid import entries preselected",
     [("apps/desktop/src/features/tenants/profile-import.tsx",
       "setSelected(view.candidates.filter((c) => c.problem === null).map((c) => c.index))",
       "setSelected(view.candidates.map((c) => c.index))")],
     FE, "shows the native preview faithfully and sends only the explicit selection"),
]


def run(cmd):
    try:
        p = subprocess.run(cmd, cwd=REPO, env=ENV, capture_output=True, text=True, timeout=600)
    except subprocess.TimeoutExpired:
        return "timeout", ""
    return p.returncode, p.stdout + p.stderr


def apply(edits):
    saved = {}
    for path, old, new in edits:
        full = os.path.join(REPO, path)
        text = saved.get(full) and open(full).read() or open(full).read()
        saved.setdefault(full, text)
        assert text.count(old) == 1, (path, old)
        with open(full, "w") as f:
            f.write(text.replace(old, new))
    return saved


def restore(saved):
    for full, text in saved.items():
        with open(full, "w") as f:
            f.write(text)


results = []
for name, edits, cmd, test in PROBES:
    saved = apply(edits)
    try:
        code, out = run(cmd)
    finally:
        restore(saved)
    results.append({"mutation": name, "files": sorted({e[0] for e in edits}), "command": cmd, "guard": test,
                    "exit_code": code, "guard_failed": code != 0 and test in out})
    print(name, code, results[-1]["guard_failed"], flush=True)
for cmd in (CORE, CORE_LIB, KEYCHAIN, FE):
    code, _ = run(cmd)
    results.append({"baseline": cmd, "exit_code": code, "passes": code == 0})
    print("baseline", cmd[-1], code, flush=True)
with open(sys.argv[2], "w") as f:
    json.dump(results, f, indent=2)
