# C32 remediation after attempt-03 (9/8/9/8/8, fail)

The composer was redesigned rather than patched: one box per tenant and workspace (its stable ID), a per-scope "Not sent" list, and a submission that advances its stage before anything asynchronous.

| Finding | Remediation | Where |
|---|---|---|
| R1 a retired conversation's draft unreachable | The box belongs to the tenant and workspace, not to a conversation: a conversation retired by a switch leaves the box as it was, and the return (new context, new workspace reference, no conversation) shows it; the new-investigation choice is kept per scope too. Nothing is restored to the core and nothing is sent. Test: "keeps the box and the new-investigation choice by workspace ID across navigation and an A/B/A that retires the conversation" | `harness.tsx`, `store.ts` |
| R2 edits lost across the starting snapshot | With one box there is nothing to move between drafts: edits on either side of the starting snapshot stay in it, and admission clears it only if it still holds the submitted text. Test: "keeps every edit across the starting snapshot" | `harness.tsx` |
| R3 untruthful recovery status | Every retirement (refused start or clear, failure, end, ending, replacement, refused delivery) keeps the text: when the box no longer holds it, it is kept under "Not sent" (per scope; "Put it in the box" appends, "Discard" removes; nothing is sent by itself), and the status names exactly where it is. A returned message from the core does the same beside a draft. Test: "says truthfully where an unsent first message is", "keeps both a returned message and the draft typed meanwhile" | `harness.tsx`, `copy.ts`, `store.ts` |
| R4 a cleared snapshot restarting the continuation | A submission has an id and its stage advances synchronously (`ending` → `clearing` → `requesting` → `starting`) before each await; every completion checks it still holds the current submission. A snapshot arriving during clearing or requesting re-enters nothing. Test: "one Send starts at most one conversation and delivers once, even when the clear publishes before its receipt" | `harness.tsx` |

B1/E1 (the C29 live Vulture verification receipt) stay owner-gated (`evidence/C29/live-scope-request.md`).
