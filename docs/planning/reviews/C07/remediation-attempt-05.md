# C07 remediation after attempt-05

Verdict attempt-05 scored architecture 9, behavior 9, authority/security 9,
validation 8 and overall 8. It had no required fixes, no blockers, and one
evidence gap (E1).

## E1: the controller regression target was not run in the packet

The C07 test plan (`docs/architecture/checks.json`, C07 `frontend_commands`)
now includes
`cargo test --locked -p xtask --features test-fixtures --test shell_controller`.
`xtask test --chunk C07` therefore runs it, and the test run records its exact
command, raw output, status and inputs in the next review packet. The suite
covers:
- the owner-acceptance path;
- the existing stale or tampered receipt, fingerprint and normal-gate rejection
  cases;
- the updated shared-source and pending-inventory tests.

## Owner direction

After attempt-05 the owner said "hey it's good enough, push on forward". D032
was widened so that owner acceptance also takes evidence gaps. It still refuses
any score below 8, any blocker, and any failure other than the threshold. The
ledger records the owner's reason and every accepted failure. The test
`owner_acceptance_takes_only_the_threshold_and_is_honored_only_when_recorded`
covers it.
