# C11 follow-ups for the P03 push to 9/10 (D034)

C11 completed on attempt-01 (8/8/8/8/8) under D034. Remediated in the tree
right after completion (land with C12's commit; re-reviewed as C11 at the
P03 push):

- R1 (high): every placement of an investigation key or sealed snapshot
  runs under one placement lock and first checks that its investigation is
  still live; a retiring credential generation marks its investigation
  retired (without blocking the authority); credential cleanup (`forget`)
  and reset's namespace purge and sessions-root removal take the same lock,
  so a write in progress finishes before removal and nothing retired is
  placed afterwards. Reset clears every report book and takes the report
  writer before purging. Tests:
  `a_write_held_across_credential_removal_leaves_no_investigation_behind`
  (the investigation key's creation held at the Keychain gate across the
  removal), `a_write_held_across_reset_leaves_no_investigation_or_report_behind`
  (a held key creation and a held report write across reset).
- R2 (medium): every opened tenant keeps its own report book; writes and
  their settlements are matched by (tenant, generation), so another tenant's
  open or edits never drop or relabel a pending write. Test:
  `held_report_writes_keep_each_tenants_own_latest_changes` (fixture hook
  `Core::hold_report_writes`).
- R3 (medium): the bounded walks follow the service's pagination: a short
  or empty page that reports more (total or cursor) is continued from where
  the service says, within the registered request bound; completeness is
  claimed only on the service's last page. Test:
  `short_and_empty_pages_that_report_more_are_walked_not_taken_as_the_end`.
- R4 (medium): Sessions opens the tenant's reports whether or not a
  management client is ready; the reports and Exception Patterns tabs stay
  available and only the investigation needs the client. Tests: UI "keeps
  existing reports usable without a management client";
  `saved_reports_stay_usable_offline_without_a_management_client` (restart
  with the client removed and the service unreachable: open, edit, remove,
  undo, export, no request; a new report still needs scan metadata).
- E2: production-interface cases for a locked Keychain at persistence (kept
  in memory, storage shown unavailable, no unsealed file), a denied session
  read (sanitized denial, nothing cached), a delayed reveal outliving its
  credential (withdrawn as cancelled, nothing placed later) and an
  unreachable service (failure, no replay, explicit retry reads again).
- O1: the survivor probes were reworked: cleanup without the placement lock
  is now the removal probe, and the report-time probe uses the unit guard.
- E1 (P03 push): C11-N10 drives the production NSSavePanel (fixture switch
  `fixture-native-sheets`): owner-only save, a confirmed Replace, Cancel
  saving nothing, focus returning to the window.
