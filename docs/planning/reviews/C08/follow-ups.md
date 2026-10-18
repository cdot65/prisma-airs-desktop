# C08 follow-ups for the P03 push to 9/10 (D034)

C08 completed on attempt-01 (8/8/8/8/8) under D034. Before the P03 phase
review these are remediated and C08 is re-reviewed to a passing 9/10:

- R1 (high): retire settled-but-unobserved management results when their
  context retires (`authority/management_domain.rs`); observing an old
  operation after a tenant switch, credential replacement/removal or reset
  must refuse or return cancelled. Test: complete an A read unobserved,
  retire, observe.
- R2 (medium): do not cache binding initialization failures
  (`management.rs` BindingCell/`bind`); a locked Keychain on the first read
  must recover on the next explicit read without reconfiguration. Test:
  first vault read Locked → NotSent, no grant; unlock → next read succeeds.
- R3 (medium): Administration must never render a previous context's rows
  under the new tenant (`administration-view.tsx`, `desktop-shell.tsx`):
  key the stateful view by context or bind state synchronously. Test in a
  layout effect across A→B and credential replacement.
- E1: add `cargo test --locked -p desktop-core --features test-fixtures
  --test context_credentials` to C08's checks (shared credential paths were
  refactored).
- Commit c07cc8b's message lost the word `management` in one line (shell
  backtick substitution); D033 records the feature.

## Status (2026-10-18, after completion)

R1, R2, R3 and E1 were remediated in the working tree right after C08's
completion (they land with C09's commit and are re-reviewed as C08 at the P03
push): `observe_management` withdraws a retired context's settled result
(`withdrawn`), `BindingCell` caches only successful bindings
(`get_or_try_init`), the shell keys Administration by context, and C08's
checks run `context_credentials`. Tests:
`a_result_settled_before_its_context_retired_is_never_delivered_afterwards`,
the recovery tail of `protected_storage_failures_are_reported_without_any_fallback`,
and the remount assertions in `administration-view.test.tsx`; three new
mutation probes.
