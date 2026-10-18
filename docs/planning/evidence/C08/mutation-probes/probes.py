"""Deliberate behavior mutations for C08's management onboarding and
Runtime/DLP reads: credential separation, token-origin binding, admission,
paging and completeness, detail lookups, secret-field removal, retired
generations, contract validation and the Administration and Settings views.

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
READS_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "management_reads"]
UNIT_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "management"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_ADMIN = vitest("src/features/administration/administration-view.test.tsx")
FE_CLIENT = vitest("src/features/tenants/management-credential.test.tsx")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
FE_SHELL = vitest("src/shell/desktop-shell.test.tsx")
BASELINES = (READS_T, UNIT_T, FE_ADMIN, FE_CLIENT, FE_SNAPSHOT, FE_SHELL)

AUTHORITY = "crates/desktop-core/src/authority.rs"
DOMAIN = "crates/desktop-core/src/authority/management_domain.rs"
MGMT = "crates/desktop-core/src/management.rs"
ADMIN = "apps/desktop/src/features/administration/administration-view.tsx"
FORM = "apps/desktop/src/features/tenants/management-credential.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"
SHELL = "apps/desktop/src/shell/desktop-shell.tsx"

PROBES = [
    ("management reads use the scan capability",
     [(DOMAIN, "        let (record, mode, destination) = match &current.management {",
       "        let (record, mode, destination) = match &current.scan {")],
     READS_T, "a_scan_only_tenant_scans_and_management_never_uses_or_replaces_the_scan_key"),
    ("the secret is sent to a token origin it was not stored for",
     [(DOMAIN, "        if endpoints.token_origin().as_deref() != Some(destination.as_str()) {", "        if false {")],
     READS_T, "the_management_credential_survives_restart_bound_to_its_token_origin"),
    ("a management client is configured without a TSG",
     [(AUTHORITY, "            if !tsg_ok {\n                return Err(Refusal::CapabilityMissing);\n            }\n", "")],
     READS_T, "only_bounded_requests_from_a_tenant_with_a_tsg_are_admitted"),
    ("removing the management client retires the scan key",
     [(AUTHORITY, "                &context,\n                CredentialClass::Management,\n            )?;\n        }\n        Intent::Management",
       "                &context,\n                CredentialClass::Scan,\n            )?;\n        }\n        Intent::Management")],
     READS_T, "removal_deletion_and_reset_retire_each_capability_as_asked"),
    # Retirement is layered (the operation's cancel token, the generation's
    # SDK session and the delivery check); removing all three lets the old
    # generation's page through.
    ("a retired generation's read is neither stopped nor discarded",
     [(DOMAIN, "                (_, true, _) | (_, _, Some(_)) | (None, _, _) => ManagementResult::Failed {",
       "                (None, _, _) => ManagementResult::Failed {"),
      (DOMAIN, "                (Some(result), false, None) => result,", "                (Some(result), _, _) => result,"),
      (DOMAIN, "                b.session.cancel();\n", ""),
      (MGMT, "            () = claim.cancel.cancelled() => None,", "            () = std::future::pending::<()>() => None,")],
     READS_T, "credentials_replaced_mid_pagination_retire_the_old_pages"),
    ("a full page without a cursor is called the last page",
     [(MGMT, "                    None if items.len() < limit as usize => None,\n                    None => Some(offset + limit),",
       "                    None if items.len() < limit as usize => None,\n                    None => None,")],
     UNIT_T, "offset_pages_claim_completeness_only_from_the_service"),
    ("completeness is claimed while a cursor follows",
     [(MGMT, "                let complete = reported.is_none() && items.len() < limit as usize;",
       "                let complete = items.len() < limit as usize;")],
     UNIT_T, "offset_pages_claim_completeness_only_from_the_service"),
    ("the service's total is ignored",
     [(MGMT, "            if let Some(total) = total_items {", "            if let Some(total) = total_items.filter(|_| false) {")],
     UNIT_T, "offset_pages_claim_completeness_only_from_the_service"),
    ("a required inventory without its array reads as empty",
     [(MGMT, "        Some(Value::Null) | None if !required => Ok(Vec::new()),", "        Some(Value::Null) | None => Ok(Vec::new()),")],
     UNIT_T, "offset_pages_claim_completeness_only_from_the_service"),
    ("a walk that ends early claims the item is absent",
     [(MGMT, "            found: DetailFound::Incomplete,\n            item: None,", "            found: DetailFound::Absent,\n            item: None,")],
     READS_T, "a_walk_that_ends_before_the_inventory_claims_nothing"),
    ("a DLP denial reads as an absent item",
     [(MGMT, "                    if ManagementFailure::from_sdk(&error) == ManagementFailure::NotFound =>",
       "                    if ManagementFailure::from_sdk(&error) != ManagementFailure::Unavailable =>")],
     READS_T, "dlp_details_use_their_own_read_and_a_404_means_absent"),
    ("an API key value leaves Rust",
     [(MGMT, "const SECRET_FIELDS: [&str; 10] = [\n    \"api_key\",\n", "const SECRET_FIELDS: [&str; 10] = [\n    \"api_key_placeholder\",\n")],
     UNIT_T, "secret_fields_never_leave_and_unknown_values_stay"),
    ("nested secret fields are kept",
     [(MGMT, "            map.retain(|key, _| !SECRET_FIELDS.contains(&key.as_str()));\n            for item in map.values_mut() {",
       "            map.retain(|key, _| !SECRET_FIELDS.contains(&key.as_str()));\n            for item in map.values_mut().take(0) {")],
     UNIT_T, "secret_fields_never_leave_and_unknown_values_stay"),
    ("the presentation accepts a complete page with a continuation",
     [(CLIENT, "            ? value.nextOffset === null", "            ? true")],
     FE_SNAPSHOT, "rejects a complete page with a continuation"),
    ("a tenant without its management view is accepted",
     [(CLIENT, "        optional((f) => oneOf(VAULT_ERRORS, f), value.scan.failure) &&\n        isManagementView(value.management)",
       "        optional((f) => oneOf(VAULT_ERRORS, f), value.scan.failure)")],
     FE_SNAPSHOT, "rejects a tenant without its management view"),
    ("an incomplete inventory is shown as a known total",
     [(ADMIN, "of ${last}+`", "of ${last}`")],
     FE_ADMIN, "reads later pages, reports partial inventories and claims completeness only from the service"),
    ("a denied refresh drops the earlier rows",
     [(ADMIN, "            setFailure(MANAGEMENT_FAILURE_TEXT[result.failure])", "            setShown(null)\n            setFailure(MANAGEMENT_FAILURE_TEXT[result.failure])")],
     FE_ADMIN, "keeps rows with their age when a refresh is denied"),
    ("a reply for a superseded read is applied",
     [(ADMIN, "        if (mine !== token.current) return\n        setLoading(false)", "        setLoading(false)")],
     FE_ADMIN, "restarts from the first page when the credential is replaced mid-pagination"),
    ("a failed read is shown as an empty inventory",
     [(ADMIN, "    const empty = page !== undefined && !failure && page.count === 0 && page.offset === 0 && page.complete",
       "    const empty = (page !== undefined && page.count === 0 && page.offset === 0 && page.complete) || failure !== null")],
     FE_ADMIN, "shows the service’s empty inventory, and a failed first read is not empty"),
    ("profile details do not classify local previews",
     [(ADMIN, "                    {info.id === 'profiles' ? (", "                    {info.id === 'topics' ? (")],
     FE_ADMIN, "renders unknown values and markup as text and filters only the current page"),
    ("the client secret stays in its field while the request is awaited",
     [(FORM, "        secretField.value = ''\n", "")],
     FE_CLIENT, "saves a separate client, clearing the secret before awaiting and keeping it out of storage"),
    ("a focused control is left under the bottom fade",
     [(SHELL, "                if (rect.bottom > bottom && rect.top - (rect.bottom - bottom) >= top) window.scrollBy(0, rect.bottom - bottom)\n                else if",
       "                if (false) window.scrollBy(0, 0)\n                else if")],
     FE_SHELL, "keeps a focused page control clear of the bottom fade and the topbar, but not rail controls"),
    ("a page's last control cannot scroll clear of the fade",
     [(SHELL, "data-slot='shell-status' className='text-academy-ink-soft mt-4 pb-14 text-[13px]'",
       "data-slot='shell-status' className='text-academy-ink-soft mt-4 text-[13px]'")],
     FE_SHELL, "lists every native feature and shows unavailable destinations explicitly"),
    ("a retired context's settled result is delivered when observed later",
     [(DOMAIN, "        if record.settled\n            && self.active_ref.as_ref() != Some(&record.context)\n", "        if false\n")],
     READS_T, "a_result_settled_before_its_context_retired_is_never_delivered_afterwards"),
    ("a failed binding is cached until reconfiguration",
     [(MGMT, "pub type BindingCell = Arc<OnceCell<ManagementBinding>>;", "pub type BindingCell = Arc<OnceCell<ManagementBinding>>;\nstatic POISON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);"),
      (MGMT, "                    Err(VaultError::Locked) => return Err(ManagementFailure::CredentialLocked),",
       "                    Err(VaultError::Locked) => {\n                        POISON.store(true, std::sync::atomic::Ordering::SeqCst);\n                        return Err(ManagementFailure::CredentialLocked);\n                    }\n                    _ if POISON.load(std::sync::atomic::Ordering::SeqCst) => return Err(ManagementFailure::CredentialLocked),")],
     READS_T, "protected_storage_failures_are_reported_without_any_fallback"),
    ("Administration keeps its instance across contexts",
     [(SHELL, "                    key={snapshot.active?.context ?? 'none'}\n", "")],
     FE_ADMIN, "restarts from the first page when the credential is replaced mid-pagination"),
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
