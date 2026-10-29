# C32 remediation after attempt-02 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 drafts keyed by a reissued reference; fresh choice in component state | Drafts and the new-investigation choice live in the shell store keyed by tenant and the stable workspace ID (`harnessDrafts`, `harnessFresh`), not the reference the core reissues on each switch. Test: "keeps drafts and the new-investigation choice by workspace ID across reissued references, navigation and A/B/A" (new context and workspace reference on return) | `harness.tsx`, `store.ts` |
| R2 ended startup, replacement | A first send whose conversation fails, ends or is ending before it is ready is retired: nothing sent, the text stays in the box shown next, Send works again. A replacement conversation receives nothing; the text is put in its box (unsent) and the message says so. An unconfirmed start queues nothing. Test: "retires a first send whose conversation ends before it is ready, or whose start is unconfirmed" (plus the attempt-01 replacement test) | `harness.tsx` |
| R3 clearing later edits | Each send remembers the box's text as submitted; admission clears the box only if it still holds that revision. For a first send, later edits follow into the new conversation's box. Test: "clears only the submitted text" (first send and delivery in a running conversation) | `harness.tsx` |
| R4 returned message hidden behind a draft; Not now copy | A returned message fills an empty box; with a draft typed meanwhile both stay: the message is shown in the alert with "Put it in the box" (appends; nothing is sent), and the status claims only what happened. The renewal note now says Not now restarts nothing and returns the message. Test: "keeps both a returned message and the draft typed meanwhile" | `harness.tsx`, `copy.ts` |
| Optional | `harness_tools::a_confirmation_that_outlives_its_conversation_runs_nothing`: the sheet held open, the conversation ended, the confirmation is dismissed and a late acceptance runs nothing | `harness_tools/main.rs` |

B1/E1 (the C29 live Vulture verification receipt) stay owner-gated (`evidence/C29/live-scope-request.md`).
