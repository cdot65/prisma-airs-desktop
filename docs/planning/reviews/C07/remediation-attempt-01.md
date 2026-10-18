# C07 remediation after attempt-01

Verdict attempt-01: architecture 8, behavior 7, authority/security 7,
validation 7, overall 7; R1–R6, E1, E2.

## R1: cached attempts outlived Academy's authority

A kept `desktop.attempt.get` reply is stored with when it was kept and, offline,
returned as Academy sent it only while its own `lease.expiresAt` holds (clock
not behind the time it was kept) and its course is not withdrawn. Otherwise it
is answered locked (`lease_expired` or `access_revoked`) with Academy's
identifying fields and the learner's draft and results, without steps,
definition or lease (`locked_attempt`, `offline_domain.rs`). Only the signed-in
account's attempts are kept. On reconnect each kept attempt is read again (a
read, never a write to Academy). Academy's reply replaces what was kept, so its
own lock for revoked access or a withdrawn revision removes the authored
definition. An attempt Academy refuses (401/403/404) is kept locked. Local
drafts are separate records and are untouched.

- Rust `cached_attempts_lock_when_their_lease_ends_access_is_revoked_or_the_revision_is_withdrawn`:
  - real-shaped attempts with a lease and a marked authored step;
  - lease end locks offline;
  - course access revoked (403) locks;
  - a revision withdrawn while the course stays allowed takes Academy's lock,
    and the lock survives restart;
  - the local draft remains;
  - nothing is sealed in plaintext;
  - no submit, check or draft save is sent.

  `cached_attempts_open_offline_and_a_submission_is_never_replayed` now uses
  the real shape.
- Probes:
  - "a cached attempt is shown after its lease ended";
  - "Academy's lock on reconnect does not replace what was kept".

## R2: offline writes outside reset's ledger

Every offline store operation first enters its generation's write ledger
(`OfflineGen::enter`, `State.offline_writers`). This covers opening, key
creation, saves, loads, clears, removal, caching, offline answers and the
writes revalidation makes. A retired generation or a reset in progress
(`StorageHealth::Resetting`) admits none. Reset waits for admitted operations
before purging the Keychain and the offline root. Downloads leave the ledger
while Academy answers and enter it again to save. The generation holds its
notifier from creation.

- Rust `reset_waits_for_admitted_offline_work_and_nothing_survives_it`:
  - the first offline key write is held at a Keychain barrier when reset is
    requested; reset stays in progress until it is released, then completes
    with no key and no root;
  - opening kept work at launch is held on the Keychain read when reset is
    requested; after release nothing is recreated;
  - a restart, even from a document still naming the signed-in learner,
    recreates nothing.

  The test releases its barriers on drop, so a regression fails within 30 s
  instead of blocking runtime teardown.
- Probe: "reset does not wait for admitted offline work".

## R3: the latest edit could be lost

Academy's draft hook (`9f24d67c`, tests `7038bfd3`) keeps the latest unconfirmed work on the
device:
- 300 ms after each edit;
- after a failed save;
- when the lab unmounts.

Every device operation runs through one ordered queue (`onDevice`), and each
reads the work current when it runs (`syncDevice`). A save request that settles
late therefore cannot overwrite newer work.

- Academy test "keeps the latest edit, not the work an older request carried,
  after leaving the lab":
  - send A, type B while A is pending, unmount, then fail A;
  - B is on the device within 50 ms of unmount (before the 300 ms edit
    timer), and reopening restores B as saved on this device only;
  - nothing is sent.
- Academy test "device writes land in the order they were asked": the first
  device write is held; the newer one waits for it and wins.
- Probes:
  - "leaving the lab keeps nothing";
  - "work a save could not confirm is not kept";
  - "device operations run out of order".

## R4: a late device read could bring back an obsolete copy

The read on mount is the first operation in the same queue. If it finds a copy,
the hook marks it as possibly uncleared, whether or not the learner has edited
since. Every later confirmation runs after the read and clears the copy once
Academy holds the latest work. `LocalDrafts.clear` now reports whether the copy
is gone; a clear that did not complete is asked again on the next sync or when
the lab unmounts. The desktop adapter (`nativeLocalDrafts.clear`) returns the
core's confirmation.

- Academy tests:
  - "a late device read never brings back a copy older than work Academy
    confirmed": the read is released after a version-1 confirmation; the copy
    is cleared, the hook keeps version 1, and reopening shows saved work;
  - "asks again to forget a copy whose clear did not complete".
- Desktop: `host-actions.test.ts` (a clear the core did not confirm reports
  false).
- Probe: "a clear that did not complete counts as done".

## R5: store writes were not serialized

`OfflineStore` holds a commit lock. Under it run:
- the quota check;
- each commit;
- deletions;
- set-asides.

Set-asides re-read the file under the lock, so a record a writer replaced
meanwhile is not set aside. Temporary files are per write
(`.<record>.<serial>.tmp`), and leftovers are removed when the store opens
(the instance lock guarantees no other writer).

`OfflineStore::update` performs read-modify-write under the lock. Revalidation
uses it and applies a verdict only if the record still holds the revision that
was checked. A removed course is not recreated, and a course downloaded again
meanwhile keeps its own status.

Records kept aside count toward the account's bytes and records. The newest
eight (`UNREADABLE_KEPT`) are kept; older ones are removed when another is set
aside.

- Store tests:
  - `concurrent_writers_commit_whole_records_within_the_budget`: 8 threads
    behind a barrier, 20 rounds each on a shared and an own record; every
    record authenticates with one writer's whole value, the count is exact,
    and no temporary files remain;
  - `records_kept_aside_are_bounded_and_count_toward_the_budget`;
  - `an_update_sees_the_current_record_and_can_leave_it`;
  - `an_interrupted_write_leaves_the_previous_version`, which also proves its
    temporary file is removed at reopen.
- Rust `revalidation_never_overwrites_a_newer_download_or_brings_back_a_removed_one`
  delays Academy's manifest reply for both races:
  - removal during revalidation;
  - a new download (rev-2) while rev-1's newer-revision verdict is pending.
- Probes:
  - "revalidation brings back a removed download";
  - "revalidation applies an older revision's verdict to a newer download";
  - "records kept aside do not count toward the budget";
  - "concurrent writers share one temporary file without the commit lock".

## R6: assembly had no byte budget

`assemble` counts the bytes of every reply it receives (the manifest and each
unit bundle). It stops with `offlineFull` as soon as the total exceeds the
16 MiB record limit, without fetching or retaining the remaining parts.

- Rust `assembling_stops_once_a_course_cannot_fit_one_record`:
  - six unit bundles of 5 MiB each, all within Academy's cap;
  - fetching stops after four, with `offlineFull`;
  - the kept download is unchanged.
- Probe: "assembling fetches every unit whatever their size".

## E1: native oversized parts were not contract-valid

The synthetic second course (`tools/native-acceptance/academy_offline.py`) now
answers with four unit bundles of 6 MiB each. That is below Academy's 7 MiB
`MAX_BUNDLE_BYTES`, which applies to `unitBundle` as well as `bundle`. C07-N07
requires `desktop.bundle` 413, then `desktop.manifest`, then exactly three
`desktop.unitBundle` requests. Fetching stops once the course cannot fit one
record, the refusal is shown and nothing is kept. Requests are recorded in
`native/observations.json`.

## E2: the interrupted-write test never ran in a gate

Fault injection (`WriteFault`, `OfflineStore::inject`) is compiled for unit
tests as well as fixture builds (`cfg(any(test, feature = "test-fixtures"))`).
`an_interrupted_write_leaves_the_previous_version` therefore runs in the frozen
common command `cargo test --locked -p desktop-core --lib` and in the probes'
`STORE_T`. It covers a fault before the rename, reopening (previous version
read, temporary file removed) and a write after restart.
