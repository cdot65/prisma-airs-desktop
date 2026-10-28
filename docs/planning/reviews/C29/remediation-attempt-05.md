# C29 remediation after attempt-05 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 an admitted creation settling before its draft snapshot let Cancel close early | An admitted creation is remembered until the draft it produced is observed; a cancellation waits for that draft however the receipt and snapshot are ordered, discards it exactly once and closes only after the discard is admitted (or the draft is seen gone). Tests: wizard.test, receipt-before-snapshot flushed separately from the snapshot (no close, no discard until the draft appears) and snapshot-before-receipt | `wizard.tsx` |

B1/E1 (the live Vulture verification receipt) stay blocked pending the owner's manifest family (`evidence/C29/live-scope-request.md`).
