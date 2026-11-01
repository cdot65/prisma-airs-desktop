# Academy 2.1 qualification handoff

Milestone 1 (qualification tooling and fixtures) is independently qualified.
The owner chose to document hosted test-data prerequisites for milestone 2.
The presentation overhaul can proceed with these limits recorded.

The runner now requires explicit result assertions and typed references to
validated server responses. It enforces the W53/W65/W66 minimum contracts,
reset readback and certificate preservation, test-account identity, safe
receipt names and redaction. Unknown mutation effects stop for reconciliation;
Daily cleanup cannot exceed budgets or use failed dependencies. Refused avatar
uploads cannot trigger removal of the existing avatar.

## Validation and reviews

- 55 runner fixture tests and 21 unit tests passed; Clippy passed with warnings
  denied. The latest three records in [validation/run.json](validation/run.json)
  bind 178 unchanged build inputs and both test executables. Earlier failed
  attempts remain in that record.
- [Independent Codex verdict](../../reviews/academy-2.1/codex-independent-04/verdict.json):
  9/10 in architecture, behavior, authority/security, validation and overall.
- [Claude peer verdict](../../reviews/academy-2.1/attempt-07/review.json):
  architecture 8/10; all other dimensions, including overall, 9/10.
  Both reviews recommend pass with no required fixes, blockers or evidence gaps.
  Claude's optional suggestions remain unchanged in its verdict.
- [Post-review verification](verification.json) records the current fingerprints,
  evidence bindings, exact scores and limitations. These are scoped milestone
  fingerprints, not formal C25/P08 gate fingerprints.

## Hosted prerequisites

Earlier production-core calls as `airs-desktop@cdot.io` established sign-in,
a 50-course catalog read and intentionally incorrect inline grading. Their
[receipts and limits](hosted/summary.json) retain their own producer bindings.
The later repeat expired before sign-in and sent no learner program.

Hosted W53/W65/W66 qualification needs an exam-eligible designated test course,
the minimum 50-question Shift pool, positive disposable course progress and an
existing earned test certificate. None was seeded or bypassed. No hosted exam,
Daily Incident or reset/certificate-preservation pass is claimed.

Follow the [operator instructions](../../academy-2.1-follow-up.md) when that
data is available. Keep hosted grading, eligibility, timing, Daily state and
certificate issuance authoritative on the Academy service during the UI
redesign; gamification stays inside Academy. This tooling milestone changes
no formal ledger or release gate. Changes remain local alongside the existing
UI/C26 work.
