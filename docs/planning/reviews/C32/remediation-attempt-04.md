# C32 remediation after attempt-04 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 a refused follow-up lost behind a later edit | A send to a running conversation that the core refuses, or that is never delivered, keeps its exact text: in the box if the box still holds it, otherwise under "Not sent", beside whatever the operator typed meanwhile; nothing is resent. Test: "keeps a refused or undelivered follow-up beside the edit made while it was delivered" | `harness.tsx` |
| R2 unconfirmed admission called "not sent" | Outcomes are three: taken (admitted), not taken (refused, never delivered) and not confirmed (the receipt was lost). A send that was not confirmed is never called "not sent": it is listed under "Not confirmed" (the list says the desktop did not confirm whether it arrived, and to check the transcript before sending again) and leaves the list, and the box if the box holds exactly it, as soon as the observed transcript shows it. A start, clear or end that was not confirmed says that whether a conversation started is not known and that the message was sent to no conversation (true: delivery only follows a confirmed start); a conversation that then appears receives nothing by itself. Tests: "never calls an unconfirmed send not sent" (the transcript shows the message, the receipt is lost) and "a lost start receipt is never no conversation started" (a conversation appears after the lost receipt) | `harness.tsx`, `copy.ts`, `store.ts` |

The attempt-03 regression that publishes the cleared state before the clear's receipt now publishes it without nesting React's act inside the scripted dispatch (the nesting left the next test's render queue pending).

B1/E1 (the C29 live Vulture verification receipt) stay owner-gated (`evidence/C29/live-scope-request.md`).
