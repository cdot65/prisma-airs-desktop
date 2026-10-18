# C07 remediation after attempt-04

Verdict attempt-04 scored 8 in every dimension (architecture, behavior,
authority/security, validation, overall), with two follow-ups (R3, R5) and no
evidence gaps.

## R3 follow-up: a visible commit is not a durable one

`OfflineStore` tracks records that were renamed into place but whose
directory sync did not complete (`unconfirmed`). `OfflineStore::durable`
reports false for such a record. It confirms the record only when a later
directory sync completes, and a retry that finds identical bytes does not
confirm it while syncs keep failing. Opening a store syncs its directory
first, so whatever an earlier run renamed into place is durable before
anything relies on it. A directory that cannot be synced stays closed.
`clear_draft` forgets lab work kept on this Mac only once the kept attempt
holds that work and the attempt's commit is known durable. Fixture builds
can make the store's directory syncs fail
(`WriteFault::DirectorySync`, persisting until cleared;
`Core::inject_offline_fault`).

- Store test
  `a_commit_whose_directory_sync_failed_is_not_durable_until_one_completes`:
  the record is visible but not durable, still not durable on retry, and a
  completed sync confirms it.
- Rust `a_confirmation_whose_commit_is_not_known_durable_leaves_the_local_copy`:
  - Directory syncs fail while Academy confirms the save.
  - The kept attempt reads the new version, but the copy is not forgotten,
    on asking or asking again.
  - After restart the newest work loads, the lease is unchanged, and the
    clear succeeds once opening has synced.
  - Nothing is replayed.
- Probe: "a local copy is forgotten though the attempt's commit is not known
  durable".

## R5 follow-up: stamps issued before opening finished

A generation signals when opening finishes, whatever the outcome
(`set_seeded`). From then on its stamps follow every persisted stamp.
Foreground `desktop.attempt.get`/`list` reads, whose results are kept, take
their stamp only after that signal (`observe_seeded`, bounded wait). Other
calls do not wait. Downloads stamp after opening, and revalidation runs
after opening, so ordering between concurrent requests is unchanged: each
stamp is still taken when its read starts, later than any persisted stamp.
Revalidation also skips re-reading attempts when Academy's access check
could not be made.

- Rust test
  `an_attempt_read_during_opening_keeps_academys_lock_over_kept_stamps_ahead_of_the_clock`:
  - The kept attempt's stamp is seeded ten years ahead, with its lease
    still valid.
  - Opening is held on the Keychain read while a foreground attempt read is
    admitted.
  - Background revalidation is unavailable, because Academy's access check
    fails.
  - Opening is released, and Academy's lock is returned and kept: offline,
    and again after restart.
- Probe: "an attempt read stamps before opening finished".

## Probe follow-up

Two probes survived the attempt-03 set:
- "revalidation applies an earlier verdict to a newer download";
- "an older revalidation verdict applies to a same-revision re-download".

Both survived because the course's `validated` stamp, written by downloads
since R2, now also guards their scenarios. Each probe now removes every
check it depends on.
