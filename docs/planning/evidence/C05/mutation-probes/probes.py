"""Deliberate behavior mutations for C05's learner host, guest/account
generations, truthful denials and truth exclusion.

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
LEARNER_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "academy_learner"]
XTASK = ["cargo", "test", "--locked", "-p", "xtask", "--lib"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_LINK = vitest("src/features/academy/trpc-link.test.ts")
FE_VIEW = vitest("src/features/academy/academy-view.test.tsx")
FE_SHELL = vitest("src/shell/desktop-shell.test.tsx")
TRUTH = ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.truth.config.ts"]

DOMAIN = "crates/desktop-core/src/authority/learner_domain.rs"
LEARNER = "crates/desktop-core/src/learner.rs"
LINK = "apps/desktop/src/features/academy/desktop-link.ts"
PROVIDER = "apps/desktop/src/features/academy/learner-provider.tsx"
VIEW = "apps/desktop/src/features/academy/academy-view.tsx"
APP = "apps/desktop/src/app.tsx"
GRAPH = "tools/academy/truth-graph.mjs"
MOMENTS = ".work/academy/src/features/units/components/completion-moments.tsx"
KC_RUNNER = ".work/academy/src/features/units/components/knowledge-check-runner.tsx"
HOST = "apps/desktop/node_modules/@ai-security-academy/learner/src/host.tsx"
FINGERPRINT = "crates/xtask/src/fingerprint.rs"
UNIT_QUERIES = ".work/academy/src/features/units/hooks/use-unit-queries.ts"
FE_GENERATIONS = vitest("src/features/academy/academy-generations.test.tsx")
BUDGETS = "crates/xtask/src/budgets.rs"
NOTICES = "crates/xtask/src/notices.rs"

PROBES = [
    ("public procedures admitted as mutations",
     [(DOMAIN, "let public = crate::learner::is_public(procedure) && kind == ProcedureKind::Query;",
       "let public = crate::learner::is_public(procedure);")],
     LEARNER_T, "a_guest_is_refused_everything_outside_the_public_queries_before_sending"),
    ("signed-in public-only calls carry the account credential",
     [(DOMAIN, "let session = if authenticated {", "let session = if signed_in {")],
     LEARNER_T, "a_signed_in_learner_sends_the_credential_only_on_the_account_allowlist"),
    ("an account procedure joins the public set",
     [(LEARNER, '    "paths.list",\n    "units.get",', '    "paths.list",\n    "progress.get",\n    "units.get",')],
     LEARNER_T, "a_guest_is_refused_everything_outside_the_public_queries_before_sending"),
    ("sign-in keeps the guest generation",
     [(DOMAIN, '        let starts = state.reissue_learner();\n        let learner = state.learner_ref.clone().expect("issued");',
       '        let starts = Vec::new();\n        let learner = state.learner_ref.clone().expect("issued");')],
     LEARNER_T, "each_account_change_issues_a_new_generation_and_retires_the_old_one"),
    ("guest refusals read as desktop limits",
     [(LINK, "const guest = options.signedIn ? !options.signedIn() : false", "const guest = false")],
     FE_LINK, "asks a guest to sign in"),
    ("signed-in refusals read as Academy's grant gate",
     [(LINK, ": desktopError(REFUSAL_TEXT.capabilityMissing!, 'METHOD_NOT_SUPPORTED',",
       ": desktopError(REFUSAL_TEXT.capabilityMissing!, 'FORBIDDEN',")],
     FE_LINK, "asks a guest to sign in"),
    ("the learner subtree survives a generation change",
     [(PROVIDER, "return <Generation key={props.learner} {...props} />", "return <Generation {...props} />")],
     FE_VIEW, "starts a fresh cache and route for each account generation"),
    ("external lesson links routed into the window",
     [(VIEW, "if (url.origin === window.location.origin) {",
       "if (url.origin === window.location.origin || url.protocol === 'https:') {")],
     FE_VIEW, "routes learner paths in place and keeps other addresses out of the window"),
    ("labs run instead of previewed",
     [(PROVIDER, "labs='preview'", "labs='run'")],
     FE_VIEW, "previews a lab instead of mounting a runner"),
    ("diagrams rendered live",
     [(PROVIDER, "diagrams='prerendered'", "diagrams='live'")],
     FE_VIEW, "shows pre-rendered diagrams without loading the diagram renderer or video"),
    ("a completed reset keeps Academy presentation storage",
     [(APP, "            forgetAcademyPresentation()\n", "")],
     FE_SHELL, "forgets Academy presentation storage when a reset completes"),
    ("failed pages show the router's raw error view",
     [(VIEW, "        defaultErrorComponent: PageFailed,\n", "")],
     FE_VIEW, "shows a plain failure view without the error text when a learner page throws"),
    ("retired course slugs are not redirected",
     [(VIEW, "if (current) throw redirect(", "if (current && false) throw redirect(")],
     FE_VIEW, "resolves a retired course slug before loading anything"),
    ("the measurement probe renders in production builds",
     [(VIEW, "{snapshot.fixtureBuild ? (", "{true ? (")],
     FE_VIEW, "offers the learner call probe only in fixture builds"),
    ("an Academy server module ships",
     [(VIEW, "import { LearnerProvider } from './learner-provider'\n",
       "import { LearnerProvider } from './learner-provider'\nimport '@/server/services/option-shuffle'\n")],
     TRUTH, "ships no Academy truth, privileged module or unexplained procedure"),
    ("a refused procedure loses its reason",
     [(GRAPH, "    'telemetry.track': 'award toasts fire it and forget it; the desktop sends no Academy product telemetry and the refusal shows nothing',\n", "")],
     TRUTH, "ships no Academy truth, privileged module or unexplained procedure"),
    ("the registered workload table is misread",
     [(BUDGETS, 'if cells.len() < 5 || cells[0] == "Budget"', 'if cells.len() < 6 || cells[0] == "Budget"')],
     XTASK, "the_registered_workload_table_parses_exactly"),
    ("a workload budget over its limit passes",
     [(BUDGETS, "Some(value) if value <= budget.limit => {", "Some(value) if value <= budget.limit * 2.0 => {")],
     XTASK, "a_budget_fails_over_its_limit_short_of_samples_or_in_another_unit"),
    ("legacy license arrays are ignored",
     [(NOTICES, 'return types.join(" OR ");', "return String::new();")],
     XTASK, "legacy_and_reviewed_file_licenses_count_only_when_exact"),
    ("focus stays on the Academy region after its page renders",
     [(VIEW, "            if (heading) {\n                focusHeading(heading)\n            } else if (tries++ < 300) {",
       "            if (heading && false) {\n                focusHeading(heading)\n            } else if (tries++ < 300) {")],
     FE_SHELL, "hands focus to the Academy page heading once the page renders"),
    ("completion state survives a learner change",
     [(MOMENTS, "    const kept = momentQueue.filter((m) => m.owner === next)", "    const kept = momentQueue"),
      (MOMENTS, "    const queue = allQueued.filter((m) => m.owner === session)", "    const queue = allQueued")],
     FE_VIEW, "never shows or watches a previous account completion in the next generation"),
    ("the host reports no mutation failures",
     [(HOST, "                    toastMutationError(event.action.error, event.mutation.meta)\n", "")],
     FE_VIEW, "asks a guest to sign in when completing a unit, which the desktop never sends"),
    ("an unknown submission outcome reads as a plain failure",
     [(KC_RUNNER, "                        mayHaveApplied:\n                            \"Academy may have recorded this submission;",
       "                        mayHaveApplied:\n                            \"Could not submit this check;")],
     FE_VIEW, "says a knowledge-check submission may have been recorded when the server fails after receiving it"),
    ("installed learner and shared copies escape verification",
     [(FINGERPRINT, "pub const SHARED_PACKAGES: [(&str, &str); 3] = [", "pub const SHARED_PACKAGES: [(&str, &str); 1] = ["),
      (FINGERPRINT, '    ("learner", "packages/learner"),\n    ("shared", "packages/shared"),\n', "")],
     XTASK, "every_installed_academy_package_is_fingerprinted_and_verified"),
    ("the learner generation is not Academy's session",
     [(PROVIDER, "({ userId: accountId, session: learner })", "({ userId: accountId })")],
     FE_GENERATIONS, "does not bring back an earlier generation after reset and signing in to the same account"),
    ("an unknown outcome is not read back",
     [(UNIT_QUERIES, "    if (mutationFailure(error) === 'mayHaveApplied') invalidateAfterUnitProgress(utils)", "    void error")],
     FE_VIEW, "says a knowledge-check submission may have been recorded when the server fails after receiving it"),
    ("guest public reads require an account session",
     [(DOMAIN, "let session = if authenticated {", "let session = if signed_in || crate::learner::is_public(procedure) {")],
     LEARNER_T, "a_guest_reads_the_public_catalog_without_any_credential"),
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
for name, edits, cmd, test in PROBES:
    saved = apply(edits)
    try:
        code, out = run(cmd)
    finally:
        restore(saved)
    results.append({"mutation": name, "files": sorted({e[0] for e in edits}), "command": cmd, "guard": test,
                    "exit_code": code, "guard_failed": code != 0 and test in out})
    print(name, code, results[-1]["guard_failed"], flush=True)
for cmd in (LEARNER_T, XTASK, FE_LINK, FE_VIEW, FE_SHELL, FE_GENERATIONS, TRUTH):
    code, _ = run(cmd)
    results.append({"baseline": cmd, "exit_code": code, "passes": code == 0})
    print("baseline", cmd[-1], code, flush=True)
with open(sys.argv[2], "w") as f:
    json.dump(results, f, indent=2)
