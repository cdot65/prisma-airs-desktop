# C32 remediation after attempt-05 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 an older identical entry settling an unconfirmed send | An unconfirmed send records the conversation it went to and how many operator messages that conversation showed before it. It is settled only by an operator message with its text that the same conversation added after that point; an older identical message, or another conversation's, settles nothing, and the text and the "Not confirmed" record stay. The observation-before-receipt case still settles. Test: "settles an unconfirmed send only by a message its own conversation added after it" (repeated opening question, another conversation showing the words) | `harness.tsx`, `store.ts` |
| R2 a refused record hiding a later uncertainty | A kept message is recorded per attempt: a later outcome for the same text replaces the earlier record, so a retry whose receipt is lost moves it from "Not sent" to "Not confirmed" and the status names the list it is in. Test: "a retry whose receipt is lost moves a refused message from Not sent to Not confirmed, truthfully" | `store.ts` |

B1/E1 (the C29 live Vulture verification receipt) stay owner-gated (`evidence/C29/live-scope-request.md`).
