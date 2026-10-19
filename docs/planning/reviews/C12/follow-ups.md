# C12 follow-ups for the P03 push to 9/10 (D034)

C12 completed on attempt-01 (8/8/8/8/8) under D034. Remediated in the tree
for the P03 push (re-reviewed as C12 there):

- R1 (high): a Runtime Security case never carries a payload. `validate`
  refuses one, so an import or catalog with such a case is refused before
  the library changes, and a stored library holding one does not open; the
  renderer contract and the producer now agree. Tests: unit
  `exports_and_imports_keep_v1s_file_and_refuse_foreign_or_malformed_files`
  and `imported_runtime_cases_with_a_payload_and_inconsistent_charts_stay_in_contract`.
- R2 (medium): the violating share is reported only for consistent totals;
  inconsistent totals keep the gap and an unknown share, never a value the
  wire contract refuses. Tests: unit `activity_needs_known_totals` and the
  same integration case.
- R3 (medium): a deleted tenant's library key enters the durable cleanup
  ledger with the deletion; the ledger drain removes the key and the sealed
  file (named after the key record) and its set-aside copy, keeping the
  entry until every part is gone; library writes check and write under the
  placement lock the drain takes. Test:
  `a_failed_library_cleanup_stays_ledgered_and_finishes_later` (denied
  Keychain, then an undeletable file, across restarts).
- R4 (medium): a deadline ticker re-evaluates time-based overview state
  (freshness, snooze ends; robust to sleep) and publishes when it changed.
  Test: `freshness_and_snooze_deadlines_are_published_without_any_event`
  (fixture clock).
- R5 (medium): saves, copies, imports and catalog additions report success
  only when their write settles; queued additions report the merge or write
  failure to the operation's originator (`CaseProblem::NotSaved`); the
  scan form shows Saving… until then and keeps its fields; `SaveLibraryAgain`
  retries a failed write. Tests: `queued_saves_and_copies_report_their_real_outcome`,
  `the_library_works_offline_survives_restart_and_retries_a_failed_save`,
  UI "reports a scan-form save only once its outcome is established".
- E1: C12-N10 drives the production NSOpenPanel/NSSavePanel (fixture switch
  `fixture-native-sheets`): owner-only export, import, cancel, a file over
  256 KiB refused, focus returning to the window.
- E2: offline Library use across restart, delayed import completion bound to
  its tenant across a switch, failed protected writes with explicit retry,
  and cleanup recovery (cases above).

## Attempt-02 (D034 raise) remediation

- E3: the failing `library_observation` baseline was two real test races,
  reproduced in the mutation copy and the checkout (exit 101, about one run
  in two). (1) A held write's success notice replaced a later refusal: an
  import queued "examples added", a malformed catalog then set
  `CaseProblem::Catalog`, and the delayed write's success cleared it. Every
  library operation now advances a per-library status stamp; an outcome
  carries its originator's stamp and a success is shown only if nothing was
  done there since (a failure to save is always shown). Test:
  `catalogs_import_examples_once_and_report_unusable_catalogs` holds the
  earlier write past the refusal; probe "an earlier write's success replaces
  a later refusal". (2) The test listed a write's temporary file as the
  library; the listing now skips temporary files and waits for the last
  save to settle. Six consecutive runs pass. The probe harness (C08-C12)
  runs each unmodified baseline three times first and keeps their output
  under `mutation-probes/baselines/`, and credits a mutation only when the
  named test itself reports a failure (a Rust `test ... FAILED` line or a
  Vitest failure line naming it), never a build error or another test.
- E2: C12-N11 runs the Library offline natively: with the scan and
  management services stopped the app lists the saved cases, a case authored
  offline survives a relaunch still offline, and after reconnecting no scan,
  catalog read or remote write is made on its own.

## Attempt-03 (D034 raise) remediation

- E3: the attempt-03 baseline failures were stale builds in the disposable
  copy, not the reviewed sources. Two filtered probe runs ("a 33rd case is
  accepted", "a Runtime case may carry a payload") built mutated
  `library.rs`; the restore wrote the original text and the next `rsync -a`
  gave the file the checkout's older modification time, so cargo reused the
  mutated test binaries (the logged warning shows the mutated payload
  condition). The harness (C08-C12) now records the SHA-256 of every file it
  mutates (`inputs`, the first entry of `results.json`, comparable with the
  review's source inventory), renews their modification times before any
  build, and checks each restore against those hashes. The probe "a failed
  write reports success" now names the outcome's field, so it compiles and is
  credited only by its named test.
