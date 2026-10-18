# C07 remediation after attempt-03

Verdict attempt-03: architecture 8, behavior 8, authority/security 8, validation 8, overall 8. Findings R1–R5; no evidence gaps.

## R1: a closed view's late reply could change a later view's copy

Academy's draft hook (`532809aa`) gives each attempt's device copy one owner: the most recently opened view, recorded per host store and attempt.

- A view claims the copy when it opens, before it reads the copy.
- `syncDevice` does nothing for a view that a later view has replaced.
- A closed view's own leave-time keep still runs, because it is queued before the next view opens.
- That view's late replies from Academy keep and clear nothing: a failed save no longer writes its older B, and a confirmed save no longer clears.

Evidence:

- Academy test "a closed view whose save to Academy fails late never changes a later view's kept work" runs twice: once with Academy's late answer a failure and once a success.
  - View 1 sends A, edits B and closes.
  - View 2 restores B, keeps C and closes.
  - View 1's request then answers.
  - C remains, reopening restores C, and nothing is sent.
- Probe: "a closed view's late reply changes a later view's kept work".

## R2: a slow download could override a newer withdrawal

A download records its access check as both its observation and its validation (`observed`, `validated`). Its final write is skipped when the kept course has a later observation or verdict, such as a newer download, a withdrawal or a renewal. In that case the download answers `notDownloadable`, and Academy's newer word stands.

Evidence:

- Rust `a_download_answered_after_a_newer_withdrawal_does_not_bring_the_course_back`:
  - An authorized download's bundle is held for 2.5 s while revalidation persists a withdrawal.
  - The download then answers `notDownloadable`.
  - The course stays withdrawn: not readable offline, with its attempt locked, both before and after restart.
- Probe: "a download answered after a newer withdrawal brings the course back".

## R3: the local copy could be cleared without offline continuity

`clear_draft` forgets lab work kept on this Mac only once the kept attempt holds it. That means the attempt's draft equals the local copy, or the attempt holds newer work that Academy confirmed after the copy's base version. Otherwise the copy stays and the call fails. The hook then keeps it marked as uncleared and asks again later.

The copy therefore survives in three cases:

- the confirmation could not be written (too large, account full, interrupted or unavailable commit);
- there is no kept attempt;
- the core never confirmed it.

Evidence:

- Rust `a_confirmation_the_kept_attempt_cannot_hold_leaves_the_local_copy`:
  - A kept attempt near the 16 MiB record limit plus a 900 KiB confirmed draft does not fit.
  - Academy's save succeeds, the clear is refused, and the newest work loads offline after restart.
  - The lease is unchanged and the save is not replayed.
- Rust `lab_work_kept_on_this_mac_survives_restart_and_is_never_sent_by_itself` covers the missing-cache case: a clear with no kept attempt is refused and the work stays.
- Interrupted and unavailable commits leave the kept attempt as it was, as the store's interrupted-write test shows, and the same rule then keeps the copy.
- Probe: "a local copy is forgotten though the kept attempt does not hold it".

## R4: advanced drafts kept stale results and conflicts

When a confirmed draft that differs from the kept one moves the kept attempt forward, every graded step is marked stale. This uses the shape and text Academy's `visibleResults` uses for a step whose draft changed after grading (`stale: true`, verdict pass false, "This draft changed after grading. Check this step again.").

`keep_newer_draft` keeps the newer draft, version, results and conflicts together. An older reply therefore cannot bring back a conflict that a confirmed resolve cleared.

Evidence:

- Rust `confirmed_work_marks_earlier_verdicts_stale_and_a_resolved_conflict_stays_resolved`:
  - A passing step at version 2 and a confirmed edit at version 3 produce a stale, non-passing step offline.
  - A pre-resolution read is held until after the resolve to version 4. Version 4 is kept, with no conflicts.
- Probes:
  - "an earlier verdict is shown for confirmed newer work";
  - "an older reply brings back a resolved conflict".

## R5: stamp ordering depended on the wall clock across restarts

When a generation opens, its stamp counter moves past every persisted stamp: course `observed` and `validated`, attempt and list `observed` (`rebuild`, `fetch_max`). `observe` never goes below the last stamp handed out. A clock set back after a restart therefore still yields later stamps. Lease checks keep using the wall clock.

Evidence:

- Rust `stamps_continue_after_kept_ones_when_the_clock_is_behind`:
  - Kept course and attempt stamps are seeded ten years ahead through the store with the account's key.
  - The app is restarted.
  - Academy's fresh denial withdraws the course and locks the attempt at once.
- Probe: "stamps restart below kept ones after a restart".
