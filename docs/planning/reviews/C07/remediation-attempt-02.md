# C07 remediation after attempt-02

Verdict attempt-02: architecture 8, behavior 7, authority/security 7,
validation 7, overall 7. Findings: R1–R6 and the E1 follow-up.

## R1 — device serialization ended with the view

Academy's draft hook (`50fbf437`) runs device operations in a single queue
per host store and attempt (`onDeviceQueue`, a module-level
`WeakMap<LocalDrafts, Map<attemptId, Promise>>`). Each host store belongs to
one learner generation. The queue outlives any one view of the lab, so an
operation left pending by a closed view runs before a later view's read or
write. A new generation brings a new host store, and therefore a new
queue, and the core refuses a retired generation's operations.

- Academy test: "a closed view's pending device write never replaces a
  later view's work" follows the exact sequence.
  - View 1 keeps A with a held acknowledgement, edits B and closes.
  - View 2 keeps C and closes.
  - Releasing view 1's write leaves C.
  - Reopening restores C, sending nothing.
- Probe: "a closed view's device queue ends with it".

## R2 — revalidation was not bound to what it observed

Every read of Academy that may change a kept record takes an observation
stamp when it starts (`OfflineGen::observe`, strictly increasing
wall-clock nanoseconds). This covers the foreground `desktop.attempt.get`
or `list` in `CallJob::run`, a download, course revalidation and each
attempt re-read.

Records carry the stamp they were observed at, and courses also carry the
stamp they were last `validated` at. A result is written only over a record
observed before it. As a result:
- a same-revision re-download keeps its own status;
- a newer foreground lock is not replaced by an older unlocked reply;
- the locked fallback does not renew `cachedAt`.

Any attempt reply keeps a newer kept draft (`keep_newer_draft`). Downloads
save only over copies started earlier.

- Rust test: `revalidation_results_never_undo_newer_state`. It uses
  one-shot per-request delays in the harness (`trpc_once_delays`).
  - A rev-9 manifest verdict is delayed while the course is removed and
    downloaded again at the same revision.
  - A stale unlocked attempt reply (draft version 1) is delayed while
    Academy's foreground lock and a confirmed draft (version 5) arrive.
  - After restart, offline: the course is current and readable, and the
    attempt stays locked with draft version 5 and no authored content.
- Probes:
  - "an older revalidation reply replaces a newer attempt";
  - "an older revalidation verdict applies to a same-revision re-download";
  - "a stale attempt reply takes back a newer confirmed draft".

## R3 — serving used the index

Offline decisions read the kept records themselves:
- `readable_record` checks the course record's status, lease and download
  time;
- `offline_attempt` reads the attempt's course record for withdrawal.

A verdict therefore applies as soon as it is written. Revalidation
publishes course verdicts before re-reading attempts. Index rebuilds are
serialized (`rebuilding`), so an older snapshot is never published after a
newer one.

- Rust test: `a_withdrawal_applies_at_once_while_other_checks_are_pending`.
  - The course denial is persisted while the attempt re-read is held for
    3 s, and Academy drops away again.
  - The unit read fails.
  - The cached attempt is locked (`access_revoked`), with no authored
    content.
- Probes:
  - "withdrawn content is served from the kept record";
  - "a withdrawn course does not lock its kept attempts".

## R4 — a wrong key could prune valid work

`OfflineStore::open` proves the key before anything else
(`verify_key`). It uses a sealed known value (`key.check`). For a store
without one, a record that authenticates proves the key, and an empty store
gets the check. A key that cannot be proven is refused with `BadKey`, and
nothing is read, set aside, deleted or written. Set-aside and pruning
therefore only ever run under a proven key.

- Store test: `a_wrong_key_opens_nothing_and_loses_nothing`.
  - It covers twelve drafts and a course.
  - A wrong key leaves every file byte-identical, with and without the key
    check.
  - The right key recovers all of them.
- Rust test: `a_wrong_offline_key_opens_nothing_and_loses_nothing` uses the
  production path.
  - Ten local drafts and a download are kept, and another 32-byte key is
    planted under the account's Keychain record.
  - After restart the store is `unavailable` and the sealed files are
    unchanged.
  - Restoring the original key gives ready, ten drafts and the download.
- `another_account_or_key_cannot_read_it_and_unreadable_records_are_kept`
  now expects `BadKey` for a wrong key. It still proves that set-aside
  happens under the right key.
- Probe: "a wrong key opens the store".

## R5 — confirmed drafts were not kept for offline

When `desktop.draft.save` or `desktop.draft.resolve` succeeds with
`status: saved`, the core moves the kept attempt's draft and version
forward (`confirm_draft`; resolve also clears conflicts). The lease and the
authored definition are left as Academy gave them, and nothing is
submitted.

- Rust test:
  `a_confirmed_draft_opens_offline_without_the_attempt_being_read_again`.
  - A save to version 3, and then a resolve to version 4, with no further
    attempt read.
  - After restart, offline: version 3 and its draft, with the lease
    unchanged; then version 4 with no conflicts.
  - No submission is sent.
- Probe: "a confirmed draft is not kept with the attempt".

## R6 — load during opening answered "none"

`load_draft` joins an opening in progress (`open_existing` takes the
opening lock). It answers null only when the account has no key, and
`offlineUnavailable` otherwise. The desktop adapter rejects when the core
does not answer, rather than returning null. The hook treats a rejected
read as unreadable: it neither restores nor writes or clears that copy.

- Rust test: `a_draft_asked_for_while_kept_work_is_opening_waits_for_it`.
  The Keychain read is held at launch, a load is requested, and the load
  returns the kept draft once the read is released.
- Academy test: "a kept copy that cannot be read is neither treated as
  absent nor replaced".
- Desktop: `host-actions.test.ts` checks that unreadable kept work rejects.
- Probes:
  - "a draft asked for while opening is answered as none";
  - "unreadable kept work is replaced".

## E1 follow-up — unit bundles were not whole projections

`assemble` requires each `desktop.unitBundle` part to be Academy's bundle
projection: schema 1, this course, this account and the manifest's
revision.

- Rust test: `a_unit_bundle_for_another_account_or_course_is_refused`.
  The assembly tests use full projections (`unit_part`).
- Native fixture (`academy_offline.py`): each of the four parts is the whole
  projection of one unit. It is checked in the builder to be below
  Academy's 7 MiB cap the way `bundles.ts` measures it (UTF-8 bytes of the
  JSON, 6,292,046 bytes). C07-N07 is re-recorded.
- Probe: "a unit bundle for another account is assembled".

## Probe follow-up

Three probes survived the first run of the expanded set, because a second
guard now also covers each one's scenario:
- "revalidation applies an older revision's verdict to a newer download":
  the observation stamp now also rejects it.
- "Academy's lock on reconnect does not replace what was kept": the
  course-withdrawal check also locks the attempt.
- "a stale attempt reply takes back a newer confirmed draft": the stamp also
  rejects the stale reply.

The first probe now removes both checks it depends on. The other two have
tests where only their own guard can protect:
- `an_attempt_academy_no_longer_returns_is_kept_locked`: Academy returns
  404 for the attempt while the course stays allowed.
- `an_attempt_read_that_predates_a_confirmed_save_keeps_the_newer_draft`: a
  read that started before a version-3 save answers after it with version 2.

All three probes were rerun in the same disposable copy. Their results
replace the earlier entries in `results.json`, and `probes.log` records
both runs.
