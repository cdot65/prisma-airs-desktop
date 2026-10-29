# C32 remediation after attempt-10 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 a claim used up before its send's outcome was known | A competing send's claim is provisional while the send is in flight (`HarnessClaim.pending`): its matching entry is only held back from the uncertain records, never used up. Admitted, the claim is confirmed and its entry used; refused or not confirmed, the claim is released and the entry is again available to settle exactly one uncertain record. Two identical unconfirmed sends and one entry therefore leave exactly one unresolved record in either order, also after a remount. Test: "two identical unconfirmed sends and one entry leave exactly one unresolved record in either order" (held retry, entry first, then its outcome lost); the other order, both admitted-retry orders, identical-unknown, older-entry and cross-conversation tests retained | `store.ts`, `harness.tsx`, provenance P50 |

Attempt-10's first check run kept three core-fixture flake logs, each a different timing-sensitive test outside C32 (runtime_changes confirmation, transport timeout, workbench documents); the same-input rerun passed.

B1/E1 (the C29 live Vulture verification receipt) stay owner-gated (`evidence/C29/live-scope-request.md`).
