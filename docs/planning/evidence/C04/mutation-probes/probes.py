"""Deliberate behavior mutations for C04's Academy identity and learner guards.

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
ACADEMY = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "academy_identity"]
CORE_LIB = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib"]
FE = ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts",
      "src/features/academy/learner-provider.test.tsx", "src/contracts/snapshot-validation.test.ts"]

ACADEMY_RS = "crates/desktop-core/src/academy.rs"
LEARNER = "crates/desktop-core/src/authority/learner_domain.rs"
AUTH = "crates/desktop-core/src/authority.rs"
PROVIDER = "apps/desktop/src/features/academy/learner-provider.tsx"
CLIENT = "packages/desktop-contracts/src/client.ts"
LINK = "apps/desktop/src/features/academy/desktop-link.ts"
FE_LINK = ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts",
           "src/features/academy/desktop-link.test.ts"]
PROBES = [
    ("issuer endpoints on another origin accepted",
     [(ACADEMY_RS, "                .filter(|u| u.origin().ascii_serialization() == config.issuer)\n", "")],
     ACADEMY, "declined_expired_and_misconfigured_sign_ins_fail_explicitly"),
    ("slow_down does not widen the interval",
     [(LEARNER, "Ok(Poll::SlowDown) => interval += Duration::from_secs(5),", "Ok(Poll::SlowDown) => {}")],
     ACADEMY, "slow_down_widens_the_poll_interval_by_five_seconds"),
    ("a grant after cancellation is kept",
     [(LEARNER, "let ok = s.learner.flow() == Some(flow);", "let ok = true;")],
     ACADEMY, "cancelling_a_sign_in_revokes_a_late_grant_and_the_browser_opens_only_the_issued_page"),
    ("a refused mutation is sent again",
     [(LEARNER, "            match self.kind {", "            match ProcedureKind::Query {")],
     ACADEMY, "a_refused_token_is_renewed_once_and_only_queries_are_asked_again"),
    ("procedures outside the allowlist admitted",
     [(LEARNER, "if crate::learner::procedure_kind(procedure) != Some(kind) {", "if procedure.is_empty() {")],
     ACADEMY, "learner_calls_pass_only_allowlisted_procedures_in_wire_form"),
    ("server-private fields passed to the renderer",
     [(ACADEMY_RS, "        screen(procedure, &mut data)?;\n", "")],
     ACADEMY, "errors_and_private_fields_are_projected_without_leaking_truth"),
    ("tenant switches retire learner work",
     [(AUTH, ".retire_contexts(ContextRef::PREFIX, keep.as_deref(), Instant::now());", ".retire_contexts(\"\", keep.as_deref(), Instant::now());")],
     ACADEMY, "tenant_switches_keep_the_learner_and_account_changes_reject_old_replies"),
    # Not probed: the `retired` check in sync_academy_calls is defense in
    # depth; retirement always stops a learner's open operations first.
    ("settled replies of a previous account stay observable",
     [(LEARNER, "        self.academy_calls\n            .retain(|r| !r.settled || current.as_ref() == Some(&r.learner));\n", "")],
     ACADEMY, "tenant_switches_keep_the_learner_and_account_changes_reject_old_replies"),
    ("a token rotated after sign-out is not revoked",
     [(LEARNER, "                None => {\n                    let _ = self\n                        .client\n                        .revoke(&config, &endpoints, &tokens.refresh)\n                        .await;\n                    return Err(CallFailure::Cancelled);",
       "                None => {\n                    return Err(CallFailure::Cancelled);")],
     ACADEMY, "signing_out_during_a_renewal_revokes_the_new_token_and_discards_the_reply"),
    ("a Keychain failure still signs in",
     [(LEARNER, "        put.map_err(Abandon::Failed)?;\n", "")],
     ACADEMY, "a_failed_vault_never_signs_in_and_revokes_the_grant"),
    ("sign-out after a restart revokes nothing",
     [(LEARNER, "        if auth.refresh.is_none() {\n            let reader", "        if false {\n            let reader")],
     ACADEMY, "signing_out_after_a_restart_revokes_the_saved_refresh_token"),
    ("no write-ahead ledger entry before a learner write",
     [(LEARNER, "        self.data.learner_cleanup.push(record.clone());\n        self.save_with(&before)", "        self.save_with(&before)")],
     ACADEMY, "a_crash_between_write_and_commit_leaves_no_untracked_credential"),
    ("learner cleanup runs while durability is unconfirmed",
     [(LEARNER, "            || state.durability_unconfirmed\n", "")],
     ACADEMY, "an_unconfirmed_rotation_keeps_the_previous_record_until_confirmed"),
    ("a rotation never retires the previous record",
     [(LEARNER, "                state.data.learner_cleanup.push(old.clone());\n", "")],
     ACADEMY, "an_unconfirmed_rotation_keeps_the_previous_record_until_confirmed"),
    ("error envelopes skip the private-field screen",
     [(ACADEMY_RS, "            screen(\"\", &mut error)?;\n", "")],
     ACADEMY, "error_envelopes_with_private_fields_never_reach_the_renderer"),
    ("the tRPC error drops the remote outcome of a server error",
     [(LINK, "                            shape.error.data = { ...(shape.error.data ?? {}), outcome: result.outcome }\n", "")],
     FE_LINK, "reports an HTTP 5xx on a mutation as possibly applied"),
    ("a server error on a mutation lacks the possibly-applied guidance",
     [(LINK, "                                shape.error.message = MAY_HAVE_APPLIED\n", "")],
     FE_LINK, "reports an HTTP 5xx on a mutation as possibly applied"),
    ("an unknown mutation reads as a plain network failure",
     [(LINK, "    if (outcome === 'unknown' && mutation) return MAY_HAVE_APPLIED\n", "")],
     FE_LINK, "reports a lost reply on a mutation as possibly applied"),
    ("reset leaves the learner namespace",
     [(AUTH, "                            [&worker.adapters.keychain, &worker.adapters.learner_keychain]", "                            [&worker.adapters.keychain]")],
     ACADEMY, "reset_signs_the_learner_out_and_purges_its_namespace"),
    ("the learner cache survives an account change",
     [(PROVIDER, "<QueryClientProvider key={learner} client={queryClient}>", "<QueryClientProvider client={queryClient}>")],
     FE, "gives each learner reference its own cache"),
    ("a non-HTTPS verification page is accepted",
     [(CLIENT, "                value.verificationUri.startsWith('https://') &&\n", "")],
     FE, "rejects a non-HTTPS verification page"),
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
for cmd in (ACADEMY, CORE_LIB, FE, FE_LINK):
    code, _ = run(cmd)
    results.append({"baseline": cmd, "exit_code": code, "passes": code == 0})
    print("baseline", cmd[-1], code, flush=True)
with open(sys.argv[2], "w") as f:
    json.dump(results, f, indent=2)
