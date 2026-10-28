# C30 remediation after attempt-08 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 a snoozed update's expiry not published at its deadline | The overview's time signature (`OverviewSignature`) now includes the holds in effect (acknowledgments, and snoozes whose return time has not passed) besides freshness, the face and the attention ids, so the deadline ticker publishes when any snooze ends, actionable or not, even while the read is already stale. Test: `ui_v2_inbox::a_snoozed_update_returns_at_its_deadline_and_is_published` (a normal Runtime activity update snoozed; the stale update drained from the production Shell subscription; the clock moved past the return time with no intent or read; the published snapshot shows the hold gone) | `observation_domain.rs`, `authority.rs` |

B1/E1 (the C29 live Vulture verification receipt) stay owner-gated (`evidence/C29/live-scope-request.md`).
