# C32 remediation after attempt-15 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 a contextual restart still reading its record survived New investigation | A contextual restart reading its archived record is retired whenever the operator moves on: New investigation (`newInvestigation` retires it with the hint), sending a message, opening a transcript, a new arrival or another tenant or workspace. Its late result is ignored: the receipt only acts for the hint that asked (`hint.id`), so neither a late success (no prefill, nothing moved to Not sent) nor a late failure (no message) changes the replacement draft or the new-investigation choice, and nothing is started or sent. The ordinary contextual prefill is unchanged. Test: "choosing New investigation while an archived record is read retires the contextual restart…" (late success, then late failure, each after typing a replacement question; fails without the fix); the archived, ended, unknown, unreadable and repeated-failure restart tests are retained | `harness.tsx`, provenance P51 |

B1/E1 (the C29 live Vulture verification receipt) stay owner-gated (`evidence/C29/live-scope-request.md`).

Also tightened around the same flow (not findings): a pending decision now holds a contextual restart back without dropping the hint or reading the record (`newInvestigation` and `startFromHint` check it first; the rail's New investigation likewise keeps the hint), and a read whose receipt was lost is said to be not confirmed ("…is not known yet… try again"), never "could not be read". Test: "a pending decision holds a contextual restart back without losing its hint; an unconfirmed read is never called a failure".
