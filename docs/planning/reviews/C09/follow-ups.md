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
  native part (retiring an open confirmation or secret sheet) remains for
  the P03 push.
