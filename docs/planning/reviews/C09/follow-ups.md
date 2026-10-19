# C09 follow-ups for the P03 push to 9/10 (D034)

C09 completed on attempt-01 (8/8/8/8/8) under D034. Remediated in the tree
right after completion (land with C10's commit; re-reviewed as C09 at the
P03 push):

- R1 (high): a revealed one-time key is handed to the native sheet once; the
  core keeps no copy (`SecretState::Shown`), so closing, copying or expiry
  ends it for good and a second RevealSecret is refused. Test: the key
  creation case now asserts one reveal only; the rotation case asserts a
  cleared key is never shown.
- R2 (medium): `settle_confirmation` re-checks the review lifetime
  (`CoreConfig::review_lifetime`); an accepted confirmation that outlived
  its review applies nothing and the review shows as expired. Test:
  `a_confirmation_that_outlives_its_review_applies_nothing`.
- R3 (medium): consumption and admission are one step; a change that cannot
  be started (full queue) leaves the review unconsumed and failed with
  `ChangeFailure::Capacity`. Test:
  `a_change_that_cannot_be_started_is_not_consumed_silently`.
- E1: `every_supported_operation_sends_exactly_v1s_request_and_reads_back`
  covers profile create/update/delete, application update and API key
  version/delete with exact method, path, query and body expectations and
  readback (the synthetic service now keeps `tsg_id` on application
  update, as the SDK's list model requires).
- E2 (core part): a draft captured while a confirmation is pending ends that
  confirmation and its late completion applies nothing
  (`a_draft_captured_while_confirming_ends_the_pending_confirmation`). The
  native part is C09-N11 (P03 push): switching the tenant through
  Accessibility while the confirmation or the one-time key sheet is open
  dismisses it; a late Return writes nothing, the clipboard is untouched and
  the key is not offered again.

## Attempt-02 (P03 re-review, 9/8/9/8/8) remediation

- R1: each secret reveal takes a generation (`SecretOwnership`); an older
  reveal's expiry and completion only end or unregister their own sheet, so
  a newer reveal stays until its own expiry, close or retirement. Tests:
  host `an_older_reveal_never_ends_or_unregisters_a_newer_sheet`; native
  C09-N12 (a second key revealed 40 s after the first was closed stays
  shown past the first reveal's 60 s expiry). The clipboard's copy-time
  deadline and owner checks are unchanged.
- E1: v1's profile previews (directional settings, the Customized
  prompt-injection mode) are refused in a profile draft before any request
  (`ChangeProblem::PreviewOnly`); legitimate service fields still go
  through. Test: `preview_only_profile_options_are_refused_before_any_request`
  (production authority-to-transport path, no preview field on the wire).
