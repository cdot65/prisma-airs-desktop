"""Deliberate behavior mutations for C06's graded workflows, host
capabilities, native host actions and learner authority.

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
PROGRAM_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "academy_program"]
AVATAR_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "avatar"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_INTERACT = vitest("src/features/academy/learner-interactions.test.tsx")
FE_ROUTES = vitest("src/features/academy/learner-routes.test.tsx")
FE_VIEW = vitest("src/features/academy/academy-view.test.tsx")
TRUTH = ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.truth.config.ts"]
# The pinned Academy input's hermetic suite (installed by the shared gate).
ACADEMY_DRAFT = ["pnpm", "--dir", ".work/academy", "exec", "vitest", "run", "--config", "vitest.presentation.config.ts",
                 "src/features/labs/desktop/__tests__/use-desktop-draft.test.tsx"]

AUTHORITY = "crates/desktop-core/src/authority.rs"
DOMAIN = "crates/desktop-core/src/authority/learner_domain.rs"
LEARNER = "crates/desktop-core/src/learner.rs"
PROVIDER = "apps/desktop/src/features/academy/learner-provider.tsx"
VIEW = "apps/desktop/src/features/academy/academy-view.tsx"
GRAPH = "tools/academy/truth-graph.mjs"
AVATAR = "crates/desktop-core/src/avatar.rs"
DRAFT = ".work/academy/src/features/labs/desktop/use-desktop-draft.ts"

PROBES = [
    ("a server error after receipt reads as rejected",
     [(DOMAIN, "            _ => Outcome::Unknown,\n        };\n        self.leave(reply);",
       "            _ => Outcome::Rejected,\n        };\n        self.leave(reply);")],
     PROGRAM_T, "a_failed_submission_reports_an_unknown_outcome_once_and_reconciles_by_query"),
    ("an earlier generation's reply is delivered after an account change",
     [(DOMAIN, "            () = claim.cancel.cancelled() => Err(CallFailure::Cancelled),\n        })",
       "            () = std::future::pending::<()>() => Err(CallFailure::Cancelled),\n        })"),
      (DOMAIN, "let retired = current.as_ref() != Some(&record.learner);\n            let result = match (reply, retired, settlement.stop) {",
       "let retired = false;\n            let result = match (reply, retired, settlement.stop.filter(|_| false)) {")],
     PROGRAM_T, "a_late_unauthorized_reply_for_an_earlier_generation_never_retires_the_newer_account"),
    ("an assessor procedure joins the learner allowlist",
     [(LEARNER, '    ("exam.getActive", Query),\n', '    ("exam.getActive", Query),\n    ("exam.grade", Mutation),\n')],
     PROGRAM_T, "assessor_grading_and_award_procedures_are_refused_before_any_request"),
    ("a print sheet that could not open reads as opened",
     [(DOMAIN, "                if opened {\n", "                if true {\n")],
     PROGRAM_T, "a_print_sheet_that_cannot_open_says_so_and_changes_nothing"),
    ("avatar uploads go to any HTTPS destination",
     [(AVATAR, "        && (reviewed_store || origin(&url) == academy_origin))", "        && (reviewed_store || !academy_origin.is_empty()))")],
     PROGRAM_T, "an_avatar_never_goes_to_an_unreviewed_destination_or_past_a_refused_step"),
    ("any address is read back as an avatar",
     [(AVATAR, "        && origin(&url) == academy_origin\n        && url.username().is_empty()\n        && url.password().is_none()\n        && url.fragment().is_none()\n        && file_ok",
       "        && url.username().is_empty()\n        && url.password().is_none()\n        && url.fragment().is_none()\n        && file_ok")],
     PROGRAM_T, "the_avatar_is_read_back_from_academys_origin_and_re_encoded_only"),
    ("a source photo beyond the 16 MiB cap is uploaded",
     [(AVATAR, "    if bytes.is_empty() || bytes.len() > limit {", "    if bytes.is_empty() {")],
     PROGRAM_T, "an_avatar_upload_sends_nothing_when_cancelled_or_the_photo_does_not_decode"),
    ("decoded pixels are allocated before the memory budget is checked",
     [(AVATAR, "    limits.reserve(decoder.total_bytes()).ok()?;\n", "")],
     PROGRAM_T, "an_avatar_upload_sends_nothing_when_cancelled_or_the_photo_does_not_decode"),
    ("resampling is not reserved from the memory budget",
     [(AVATAR, "    limits\n        .reserve(u64::from(side) * u64::from(AVATAR_SIZE) * 16)\n        .ok()?;\n", "")],
     AVATAR_T, "decoded_pixels_and_their_resampling_together_stay_within_the_budget"),
    ("a photo that does not decode is uploaded as chosen",
     [(AVATAR, "    reencode(bytes, MAX_SOURCE_BYTES).map(|(_, jpeg)| jpeg)", "    Some(bytes.to_vec()).filter(|b| !b.is_empty())")],
     PROGRAM_T, "an_avatar_upload_sends_nothing_when_cancelled_or_the_photo_does_not_decode"),
    ("a lost store reply reads as nothing stored",
     [(DOMAIN, "            crate::academy::StoreOutcome::Uncertain => {\n                leave(&self.slot, Err(CallFailure::UploadFailed));\n                return Outcome::Unknown;",
       "            crate::academy::StoreOutcome::Uncertain => {\n                leave(&self.slot, Err(CallFailure::UploadFailed));\n                return Outcome::Rejected;")],
     PROGRAM_T, "an_upload_that_may_have_replaced_the_photo_is_reported_as_possibly_applied"),
    ("a refused confirmation after a stored photo reads as nothing changed",
     [(DOMAIN, "        let outcome = match &reply {\n            Ok(CallReply::Data(_)) => Outcome::Completed,\n            _ => Outcome::Unknown,\n        };",
       "        let outcome = match &reply {\n            Ok(CallReply::Data(_)) => Outcome::Completed,\n            Ok(CallReply::Error { status, .. }) if (400..500).contains(status) => Outcome::Rejected,\n            _ => Outcome::Unknown,\n        };")],
     PROGRAM_T, "an_upload_that_may_have_replaced_the_photo_is_reported_as_possibly_applied"),
    ("the photo at an unchanged address is not read again",
     [(".work/academy/src/features/settings/hooks/use-avatar-upload.ts", "                refreshLearnerImages()\n", "")],
     FE_INTERACT, "reads the photo Academy serves again after an upload that may have replaced it at the same address"),
    ("the assessment timer is not rendered",
     [(VIEW, "                <LearnerStatus />\n", "")],
     FE_INTERACT, "shows Academy's countdown in view and submits once at the deadline"),
    ("overlapping lab draft saves",
     [(DRAFT, "        const run = queue.current.then(saveOnce, saveOnce)", "        const run = saveOnce()")],
     ACADEMY_DRAFT, "never overlap; a later save carries the newer work on the confirmed version"),
    ("course reset is not offered",
     [(PROVIDER, "                    courseReset\n", "")],
     FE_INTERACT, "resets course progress only after a confirmation with the counts and certificates kept"),
    ("completed lessons ask for no rating",
     [(PROVIDER, "                    lessonFeedback\n", "")],
     FE_INTERACT, "asks for a rating on a completed lesson and sends it once"),
    ("the avatar is uploaded through the page instead of the core",
     [(PROVIDER, "                    onUploadAvatar={onUploadAvatar}\n", "")],
     FE_INTERACT, "uploads a new photo natively, re-reads Academy and shows the photo the core reads back"),
    ("the uploaded photo is not read back",
     [(PROVIDER, "                    loadImage={loadImage}\n", "")],
     FE_INTERACT, "uploads a new photo natively, re-reads Academy and shows the photo the core reads back"),
    ("Home claims commendations the desktop refuses",
     [(PROVIDER, "                    awardPresenters={false}\n", "")],
     FE_ROUTES, "/ sends only admitted procedures and shows no refusal"),
    ("certificate download uses the browser's print",
     [(PROVIDER, "                    onPrint={onPrint}\n", "")],
     FE_INTERACT, "link verification to Academy web and download through the desktop's print sheet"),
    ("verify links use the app's origin",
     [(PROVIDER, "                    webOrigin={webOrigin}\n", "")],
     FE_INTERACT, "link verification to Academy web and download through the desktop's print sheet"),
    ("account sign-out bypasses the desktop",
     [(PROVIDER, "                    onSignOut={onSignOut}\n", "")],
     FE_INTERACT, "signs out through the desktop and leaves appearance to the app"),
    ("guests see the account sections",
     [(VIEW, "LEARNER_NAV.filter((item) => signedIn || item.to === '/learn')", "LEARNER_NAV.filter(() => true)")],
     FE_INTERACT, "ask a guest to sign in at"),
    ("account routes render for guests",
     [(VIEW, "component: signedIn ? () => <>{Page()}</> : signInFirst", "component: () => <>{Page()}</>")],
     FE_INTERACT, "ask a guest to sign in at"),
    ("labs previewed instead of run through the attempt contract",
     [(PROVIDER, "labs='desktop'", "labs='preview'")],
     FE_VIEW, "runs a lab through the desktop attempt contract"),
    ("a refused beacon loses its reason",
     [(GRAPH, "    'telemetry.track': 'view, click and award beacons fire it and forget it with meta.silent; the desktop sends no Academy product telemetry and the refusal shows nothing',\n", "")],
     TRUTH, "ships no Academy truth, privileged module or unexplained procedure"),
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
for cmd in (PROGRAM_T, AVATAR_T, FE_INTERACT, FE_ROUTES, FE_VIEW, TRUTH, ACADEMY_DRAFT):
    code, _ = run(cmd)
    results.append({"baseline": cmd, "exit_code": code, "passes": code == 0})
    print("baseline", cmd[-1], code, flush=True)
with open(sys.argv[2], "w") as f:
    json.dump(results, f, indent=2)
