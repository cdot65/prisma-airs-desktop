# C32 remediation after attempt-09 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 a retry's entry arriving before its receipt settling the original | A send competing with an unconfirmed send of the same text to the same conversation claims its own transcript entry before it is dispatched (`claimHarnessEntry` returns the claim's id), so whichever arrives first, its entry or its receipt, the entry is the retry's: the earlier attempt stays "Not confirmed". The claim is withdrawn if the send is refused or never delivered, or not confirmed (then it is an uncertain record itself), and kept if admitted. Tests: "a retry whose entry arrives before its receipt never settles the unconfirmed original either" (entry published while the receipt is held, then admitted; across renders and a remount) and the receipt-first test of attempt-08 (identical-unknown, older-entry and cross-conversation tests retained) | `store.ts`, `harness.tsx` |

B1/E1 (the C29 live Vulture verification receipt) stay owner-gated (`evidence/C29/live-scope-request.md`).
