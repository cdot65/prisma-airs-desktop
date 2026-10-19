# C10 follow-ups for the P03 push to 9/10 (D034)

C10 completed on attempt-01 (8/8/8/8/8) under D034. Remediated in the tree
right after completion (land with C11's commit; re-reviewed as C10 at the
P03 push):

- R1 (medium): the dictionary editor uses the chosen file only once one is
  actually staged (a cancelled or failed choice leaves the typed keywords in
  use), and the metadata must parse as exactly one JSON object before any
  dispatch; the body is built by serialization, never by splicing text.
  Tests: dlp-authoring.test.tsx "keeps typed keywords in use…", "refuses
  metadata that is not exactly one JSON object…".
- R2 (medium): review previews show a dictionary file as a summary (keyword
  and byte counts, SHA-256 prefix), never its text. Test:
  `a_staged_files_content_never_reaches_a_projection_and_the_pending_hold_keeps_it`
  checks snapshots and published updates before and after Review and while
  confirming.
- R3 (medium): ChooseDictionaryFile is refused while a confirmation holds
  the editor; a sheet that answers during a confirmation ends it (late
  completion applies nothing, the review fails as StagedFile); staged files
  and late answers never survive a context change. Tests: the same case and
  `a_sheet_answering_during_a_confirmation_ends_it_and_late_or_retired_choices_stage_nothing`.
- E2: `every_supported_dlp_mutation_reports_denial_unknown_and_conflict_truthfully`
  covers denial, unknown and (for existing items) baseline conflict for all
  nine supported DLP mutations, with no retry.
- Probe gap: a pattern leaf missing its confidence level (or match type,
  occurrence operator, version, id) is now a unit case.
- E1 (P03 push): C10-N10 drives the production NSOpenPanel (fixture switch
  `fixture-native-sheets`): choose, cancel, a file over the read bound
  refused, the source unchanged, focus returning to the window.

## P03 probe re-run (hardened harness)

"a cancelled file choice leaves the file option in use" survived the
hardened harness: its second edit set the file option when the sheet
opened, which the settle effect then reset (an equivalent mutant; the
earlier record showed it uncaught). The mutation now makes the settle
effect keep the file option, and "keeps typed keywords in use when a file
choice is cancelled or fails" kills it.
