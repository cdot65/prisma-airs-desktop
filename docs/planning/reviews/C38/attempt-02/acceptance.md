# C38 attempt-02 — accepted at 8 (D034)

Verdict: `verdict-returned.json` (Codex, gpt-6.1-sol, xhigh; packet `message.txt`, scope and diff measured from C36's 0605b302 at 492ab3bb): architecture 9, behavior 8, authority_security 9, validation 8, overall 8; blockers empty, evidence gaps empty, one required fix.

Under D034 a chunk completes on a review scoring at least 8 in all five dimensions with no blockers. The owner's standing direction of 2026-10-30 ("go go go … until we are 100% complete") is the acceptance; the ledger entry is written when the P09 chunks are run through `xtask review`/`xtask complete` (the controller's predecessor gate refuses P09 subjects until C26 completes; see docs/planning/status.md).

Accepted failure, to be remediated to a passing 9/10 before the P09 phase review (D034: a new generation, later chunks not reopened):

- R1: the v2 Project, MCP, History and Sign-in panels collapse an unknown delivery (`{kind:'unknown'}`) into "That was not done": the folder-permission change, an MCP sign-out, a history deletion or a Harness sign-out may already have happened. Preserve the unknown outcome as Not confirmed naming the action, direct reconciliation through the observed state, distinguish real refusal reasons from the folder-validation copy, and cover lost receipts for these actions in `harness.test.tsx` through the v2 imports with later snapshots establishing whether the change occurred (no resubmission).

Optional suggestions O1 (one owner for the initial history/catalog reads) and O2 (the parity notes' approval paragraph should name the rebuilt executable) are noted for the same generation.
