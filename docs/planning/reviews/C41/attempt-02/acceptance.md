# C41 attempt-02 — accepted at 8 (D034)

Verdict: `verdict-returned.json` (Codex, gpt-6.1-sol, xhigh; packet `message.txt`, scope and diff measured from C40's pre-gate commit 73048f76 at abc05a3b): architecture 9, behavior 8, authority_security 9, validation 8, overall 8; blockers empty, evidence gaps empty, one required fix. Attempt-01's four fixes and its evidence gap were remediated in abc05a3b (`remediation-attempt-01.md`) and are not raised again.

Under D034 a chunk completes on a review scoring at least 8 in all five dimensions with no blockers. The owner's standing direction of 2026-10-30 ("go go go … until we are 100% complete") is the acceptance; the ledger entry is written when the P09 chunks are run through `xtask review`/`xtask complete` (the controller's predecessor gate refuses P09 subjects until C26 completes; see docs/planning/status.md).

Accepted failure, to be remediated to a passing 9/10 before the P09 phase review (D034: a new generation, later chunks not reopened):

- R1: `parseRows` trims names and values on every render of the controlled Params and Headers tables, so a space typed at the end of a value is gone before the next character ("hello world" became "helloworld" in the sent query and header) and an unrelated row edit rewrote an existing padded query value (`q=  padded  ` to `q=padded`), against desktop-core's verbatim query-value contract. Preserve in-progress spaces in both tables and query-value whitespace when other rows are edited; add component regressions with character-by-character multiword entry and an existing padded value, asserting the exact Save and Send fields.
