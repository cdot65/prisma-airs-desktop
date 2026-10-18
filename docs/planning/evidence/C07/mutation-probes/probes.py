"""Deliberate behavior mutations for C07's encrypted offline work: what is
downloaded and for whom, when offline answers are given, revalidation,
locking, sealing, recovery, reset, launch-time opening, the Downloads page,
snapshot validation and lab work kept on this Mac.

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
OFFLINE_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--features", "test-fixtures", "--test", "academy_offline"]
STORE_T = ["cargo", "test", "--locked", "-p", "desktop-core", "--lib", "offline"]


def vitest(*files):
    return ["pnpm", "--filter", "@prisma-airs/desktop", "exec", "vitest", "run", "--config", "vitest.config.ts", *files]


FE_OFFLINE = vitest("src/features/academy/offline-work.test.tsx")
FE_HOST = vitest("src/features/academy/host-actions.test.ts")
FE_SNAPSHOT = vitest("src/contracts/snapshot-validation.test.ts")
# The pinned Academy input's hermetic suite (installed by the shared gate).
ACADEMY_DRAFT = ["pnpm", "--dir", ".work/academy", "exec", "vitest", "run", "--config", "vitest.presentation.config.ts",
                 "src/features/labs/desktop/__tests__/use-desktop-draft.test.tsx"]
BASELINES = (OFFLINE_T, STORE_T, FE_OFFLINE, FE_HOST, FE_SNAPSHOT, ACADEMY_DRAFT)

AUTHORITY = "crates/desktop-core/src/authority.rs"
DOMAIN = "crates/desktop-core/src/authority/learner_domain.rs"
OFFLINE = "crates/desktop-core/src/authority/offline_domain.rs"
STORE = "crates/desktop-core/src/offline.rs"
PAGE = "apps/desktop/src/features/academy/offline-work.tsx"
HOST = "apps/desktop/src/features/academy/host-actions.ts"
CLIENT = "packages/desktop-contracts/src/client.ts"
DRAFT = ".work/academy/src/features/labs/desktop/use-desktop-draft.ts"

PROBES = [
    ("a download whose lease ended is still read offline",
     [(OFFLINE, "            DownloadStatus::Current | DownloadStatus::Outdated\n        )\n        .then_some(record)",
       "            DownloadStatus::Current | DownloadStatus::Outdated | DownloadStatus::Expired\n        )\n        .then_some(record)")],
     OFFLINE_T, "an_expired_lease_is_not_read_offline"),
    ("a course Academy does not give the account is downloaded",
     [(OFFLINE, 'if access["accountId"] != generation.account() || !allowed {', 'if access["accountId"] != generation.account() {')],
     OFFLINE_T, "a_course_academy_does_not_give_the_account_is_not_downloaded"),
    ("another account's bundle is kept",
     [(OFFLINE, '        && bundle["accountId"] == generation.account()\n', "")],
     OFFLINE_T, "a_course_academy_does_not_give_the_account_is_not_downloaded"),
    ("unit bundles of different revisions are assembled into one course",
     [(OFFLINE, '            || part["revision"] != manifest["revision"]\n', "")],
     OFFLINE_T, "a_course_past_academys_bundle_cap_is_assembled_unit_by_unit"),
    ("a course Academy withdrew stays current",
     [(OFFLINE, 'let mut status = if allowed { None } else { Some("withdrawn") };', 'let mut status = if allowed { None } else { Some("current") };')],
     OFFLINE_T, "reconnecting_withdraws_courses_academy_no_longer_gives_and_marks_new_revisions"),
    ("a new revision is not marked outdated",
     [(OFFLINE, '                } else {\n                    "outdated"\n                });', '                } else {\n                    "current"\n                });')],
     OFFLINE_T, "reconnecting_withdraws_courses_academy_no_longer_gives_and_marks_new_revisions"),
    ("reconnecting does not re-check downloads",
     [(DOMAIN, "                    if offline.mark_reachable(true)\n", "                    if offline.mark_reachable(true) && false\n")],
     OFFLINE_T, "reconnecting_withdraws_courses_academy_no_longer_gives_and_marks_new_revisions"),
    ("attempt reads are not kept for offline use",
     [(DOMAIN, "                                cache.cache(&procedure, input.as_deref(), &data, observed);",
       "                                let _ = (&cache, &input, &data, observed);")],
     OFFLINE_T, "cached_attempts_open_offline_and_a_submission_is_never_replayed"),
    ("signing out leaves offline work open",
     [(DOMAIN, "        self.offline = None;\n", "")],
     OFFLINE_T, "sign_out_locks_offline_work_and_another_account_cannot_see_it"),
    ("records are written unsealed",
     [(STORE, "let sealed = self.seal(&name, &plain)?;", "let sealed = plain.clone();")],
     OFFLINE_T, "a_downloaded_course_is_kept_sealed_for_its_account_under_academys_lease"),
    ("the account budget admits twice its bytes",
     [(STORE, "    after <= ACCOUNT_LIMIT && after_count <= RECORD_COUNT", "    after <= ACCOUNT_LIMIT * 2 && after_count <= RECORD_COUNT")],
     STORE_T, "an_account_is_bounded_in_bytes_and_records"),
    ("the record cap admits one more record",
     [(STORE, "    after <= ACCOUNT_LIMIT && after_count <= RECORD_COUNT", "    after <= ACCOUNT_LIMIT && after_count <= RECORD_COUNT + 1")],
     STORE_T, "an_account_is_bounded_in_bytes_and_records"),
    ("an unreadable record is deleted instead of kept aside",
     [(STORE, '        let _ = fs::rename(\n            path,\n            self.dir.join(format!("{name}{UNREADABLE}{stamp:024}")),\n        );',
       '        let _ = (fs::remove_file(path), stamp);')],
     OFFLINE_T, "an_unreadable_record_is_kept_aside_and_the_rest_still_opens"),
    ("reset leaves offline work behind",
     [(AUTHORITY, "                        if let Some(root) = &worker.adapters.offline_root {",
       "                        if let Some(root) = None::<&std::path::PathBuf> {")],
     OFFLINE_T, "reset_removes_offline_work_and_its_keys"),
    ("installing the transport policy at launch opens offline work off the runtime",
     [(AUTHORITY, ".filter(|o| !previous.as_ref().is_some_and(|p| Arc::ptr_eq(p, o)));",
       ".filter(|_| previous.is_some() || previous.is_none());")],
     OFFLINE_T, "a_relaunch_while_academy_is_unreachable_still_opens_downloaded_work"),
    ("record ids are not checked",
     [(DOMAIN, "let id_ok = super::offline_domain::plain_id;", "let id_ok = |_: &str| true;")],
     OFFLINE_T, "only_the_signed_in_current_learner_keeps_work_offline"),
    ("the offline notice shows while Academy answers",
     [(PAGE, "if (!work || work.offline.reachable || work.offline.state === 'signedOut') return null",
       "if (!work || work.offline.state === 'signedOut') return null")],
     FE_OFFLINE, "while Academy is unreachable"),
    ("a download names the wrong course",
     [(PAGE, "{ kind: 'downloadCourse', learner: work.learner, courseId: id }",
       "{ kind: 'downloadCourse', learner: work.learner, courseId: course.title }")],
     FE_OFFLINE, "downloads a course Academy offers"),
    ("offline work is accepted in a signed-out snapshot",
     [(CLIENT, "(value.academy as Record<string, unknown>).state !== 'signedIn' && (value.academyOffline as Record<string, unknown>).state !== 'signedOut'",
       "false")],
     FE_SNAPSHOT, "offline work shown while signed out"),
    ("a kept draft that is not an object is read back",
     [(HOST, "typeof savedAt === 'string' && draft && typeof draft === 'object'", "typeof savedAt === 'string' && draft !== undefined")],
     FE_HOST, "a draft that is not an object"),
    ("kept work is sent by itself when the lab opens",
     [(DRAFT, "            setStatus('local')\n", "            setStatus('local')\n            void saveNow()\n")],
     ACADEMY_DRAFT, "without sending it"),
    ("a confirmed save does not forget the kept copy",
     [(DRAFT, "                    same(latest.current.draft, work) ? 'saved' : 'unsaved'\n                )\n                void syncDevice()\n",
       "                    same(latest.current.draft, work) ? 'saved' : 'unsaved'\n                )\n")],
     ACADEMY_DRAFT, "forgets it once Academy holds it"),
    ("work a save could not confirm is not kept",
     [(DRAFT, "const kept = await syncDevice()", "const kept = await Promise.resolve(false)")],
     ACADEMY_DRAFT, "keeps work a save could not confirm"),
    ("an older kept copy replaces work started here",
     [(DRAFT, "            if (!live || !fresh.current) return\n", "            if (!live) return\n")],
     ACADEMY_DRAFT, "keeps an older one from replacing newer work"),
    ("device operations run out of order",
     [(DRAFT, "    const run = (queues.get(attemptId) ?? Promise.resolve()).then(op, op)", "    const run = op()")],
     ACADEMY_DRAFT, "device writes land in the order they were asked"),
    ("a clear that did not complete counts as done",
     [(DRAFT, "                    uncleared.current = !(await localDrafts", "                    uncleared.current = false && !(await localDrafts")],
     ACADEMY_DRAFT, "asks again to forget a copy whose clear did not complete"),
    ("leaving the lab keeps nothing",
     [(DRAFT, "                !same(latest.current.draft, confirmed.current)\n            )\n                void syncDevice()\n",
       "                !same(latest.current.draft, confirmed.current)\n            )\n                void 0\n")],
     ACADEMY_DRAFT, "after leaving the lab"),
    ("reset does not wait for admitted offline work",
     [(AUTHORITY, "                                    + s.offline_writers", "                                    + 0 * s.offline_writers")],
     OFFLINE_T, "reset_waits_for_admitted_offline_work_and_nothing_survives_it"),
    ("a cached attempt is shown after its lease ended",
     [(OFFLINE, "} else if !lease.is_some_and(|expires| now < expires && now >= cached) {", "} else if lease.is_none() && cached == 0 {")],
     OFFLINE_T, "cached_attempts_lock_when_their_lease_ends_access_is_revoked_or_the_revision_is_withdrawn"),
    ("Academy's lock on reconnect does not replace what was kept",
     [(OFFLINE, '                        locked_attempt(&current["attempt"], "access_revoked"),', '                        current["attempt"].clone(),')],
     OFFLINE_T, "an_attempt_academy_no_longer_returns_is_kept_locked"),
    ("assembling fetches every unit whatever their size",
     [(OFFLINE, "if received > crate::offline::RECORD_LIMIT as usize {", "if received == usize::MAX {")],
     OFFLINE_T, "assembling_stops_once_a_course_cannot_fit_one_record"),
    ("revalidation brings back a removed download",
     [(OFFLINE, "                let mut record = current?;", "                let mut record = current.unwrap_or_else(|| json!({\"bundle\": {\"revision\": revision}}));")],
     OFFLINE_T, "revalidation_never_overwrites_a_newer_download_or_brings_back_a_removed_one"),
    ("revalidation applies an earlier verdict to a newer download (revision, stamp and verdict checks removed)",
     [(OFFLINE, '                if record["bundle"]["revision"] != revision.as_str()\n                    || stamp(&record) >= observed\n                    || record["validated"].as_u64().unwrap_or(0) >= observed\n',
       '                if record["bundle"]["revision"].is_null()\n')],
     OFFLINE_T, "revalidation_never_overwrites_a_newer_download_or_brings_back_a_removed_one"),
    ("records kept aside do not count toward the budget",
     [(STORE, "if !name.starts_with('.') && (name.ends_with(\".bin\") || name.contains(UNREADABLE)) {", "if !name.starts_with('.') && name.ends_with(\".bin\") {")],
     STORE_T, "records_kept_aside_are_bounded_and_count_toward_the_budget"),
    ("concurrent writers share one temporary file without the commit lock",
     [(STORE, "        let _commit = self.committing()?;\n        self.put_committing(kind, id, value)", "        self.put_committing(kind, id, value)"),
      (STORE, 'let temp = self.dir.join(format!(".{name}.{serial}.tmp"));', 'let temp = self.dir.join(format!(".{name}.tmp")); let _ = serial;')],
     STORE_T, "concurrent_writers_commit_whole_records_within_the_budget"),
    ("an older revalidation reply replaces a newer attempt",
     [(OFFLINE, "                // A read or save observed after this one already won.\n                if stamp(&current) >= observed {",
       "                // A read or save observed after this one already won.\n                if stamp(&current) >= observed && false {")],
     OFFLINE_T, "revalidation_results_never_undo_newer_state"),
    ("an older revalidation verdict applies to a same-revision re-download (stamp and verdict checks removed)",
     [(OFFLINE, '                    || stamp(&record) >= observed\n                    || record["validated"].as_u64().unwrap_or(0) >= observed\n', "")],
     OFFLINE_T, "revalidation_results_never_undo_newer_state"),
    ("a stale attempt reply takes back a newer confirmed draft",
     [(OFFLINE, "        && current[\"draftVersion\"].as_u64() > incoming[\"draftVersion\"].as_u64()", "        && false")],
     OFFLINE_T, "an_attempt_read_that_predates_a_confirmed_save_keeps_the_newer_draft"),
    ("withdrawn content is served from the kept record",
     [(OFFLINE, '            Some("current") => DownloadStatus::Current,\n', '            Some("current") => DownloadStatus::Current,\n            Some("withdrawn") => DownloadStatus::Current,\n')],
     OFFLINE_T, "a_withdrawal_applies_at_once_while_other_checks_are_pending"),
    ("a withdrawn course does not lock its kept attempts",
     [(OFFLINE, ".is_some_and(|c| Self::withdrawn_course(store, c))", ".is_some_and(|c| c.is_empty() && Self::withdrawn_course(store, c))")],
     OFFLINE_T, "a_withdrawal_applies_at_once_while_other_checks_are_pending"),
    ("a wrong key opens the store",
     [(STORE, "        store.verify_key()?;\n", "")],
     OFFLINE_T, "a_wrong_offline_key_opens_nothing_and_loses_nothing"),
    ("a confirmed draft is not kept with the attempt",
     [(DOMAIN, "                                cache.confirm_draft(&procedure, input.as_deref(), &data);",
       "                                let _ = (&cache, &input, &data);")],
     OFFLINE_T, "a_confirmed_draft_opens_offline_without_the_attempt_being_read_again"),
    ("a draft asked for while opening is answered as none",
     [(OFFLINE, "        None => match generation.open_existing() {", "        None => match OfflineState::Empty {")],
     OFFLINE_T, "a_draft_asked_for_while_kept_work_is_opening_waits_for_it"),
    ("a unit bundle for another account is assembled",
     [(OFFLINE, '            || part["accountId"] != account\n', "")],
     OFFLINE_T, "a_unit_bundle_for_another_account_or_course_is_refused"),
    ("a closed view's device queue ends with it",
     [(DRAFT, "                ? onDeviceQueue(localDrafts, attempt.attemptId, op)", "                ? onDeviceQueue(localDrafts, `${attempt.attemptId}:${Math.random()}`, op)")],
     ACADEMY_DRAFT, "pending device write never replaces a later view"),
    ("unreadable kept work is replaced",
     [(DRAFT, "            if (unreadable.current) return false\n", "")],
     ACADEMY_DRAFT, "a kept copy that cannot be read is neither treated as absent nor replaced"),
    ("a closed view's late reply changes a later view's kept work",
     [(DRAFT, "        if (owners(localDrafts).get(attempt.attemptId) !== owner.current)\n            return Promise.resolve(false)\n", "")],
     ACADEMY_DRAFT, "a closed view whose save to Academy"),
    ("a download answered after a newer withdrawal brings the course back",
     [(OFFLINE, '                stamp(c) > observed || c["validated"].as_u64().unwrap_or(0) > observed', "                stamp(c) > observed")],
     OFFLINE_T, "a_download_answered_after_a_newer_withdrawal_does_not_bring_the_course_back"),
    ("a local copy is forgotten though the kept attempt does not hold it",
     [(OFFLINE, "            if !held || !store.durable(RecordKind::Attempt, attempt_id) {", "            if !store.durable(RecordKind::Attempt, attempt_id) {")],
     OFFLINE_T, "a_confirmation_the_kept_attempt_cannot_hold_leaves_the_local_copy"),
    ("an earlier verdict is shown for confirmed newer work",
     [(OFFLINE, '                attempt["result"] = stale_results(attempt["result"].take());', '                let _ = stale_results(Value::Null);')],
     OFFLINE_T, "confirmed_work_marks_earlier_verdicts_stale_and_a_resolved_conflict_stays_resolved"),
    ("an older reply brings back a resolved conflict",
     [(OFFLINE, '        for key in ["draft", "draftVersion", "result", "conflicts"] {', '        for key in ["draft", "draftVersion", "result"] {')],
     OFFLINE_T, "confirmed_work_marks_earlier_verdicts_stale_and_a_resolved_conflict_stays_resolved"),
    ("stamps restart below kept ones after a restart",
     [(OFFLINE, "        self.observed.fetch_max(persisted, Ordering::SeqCst);", "        let _ = persisted;")],
     OFFLINE_T, "stamps_continue_after_kept_ones_when_the_clock_is_behind"),
    ("a local copy is forgotten though the attempt's commit is not known durable",
     [(OFFLINE, "            if !held || !store.durable(RecordKind::Attempt, attempt_id) {", "            if !held {")],
     OFFLINE_T, "a_confirmation_whose_commit_is_not_known_durable_leaves_the_local_copy"),
    ("an attempt read stamps before opening finished",
     [(DOMAIN, "tokio::task::spawn_blocking(move || offline.observe_seeded())", "tokio::task::spawn_blocking(move || offline.observe())")],
     OFFLINE_T, "an_attempt_read_during_opening_keeps_academys_lock_over_kept_stamps_ahead_of_the_clock"),
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
