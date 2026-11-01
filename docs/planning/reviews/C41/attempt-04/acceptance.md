# C41 attempt-04 — accepted at 8 (D034)

Verdict: `verdict-returned.json` (Codex, gpt-6.1-sol, xhigh; packet `message.txt`, scope and diff measured from C40's pre-gate commit 73048f76 at da076440): architecture 9, behavior 8, authority_security 9, validation 8, overall 8; blockers empty, evidence gaps empty, one required fix. Attempt-03's fix (the running transfer in view over an inspected run) was remediated in da076440 (`remediation-attempt-03.md`) and is not raised again.

Under D034 a chunk completes on a review scoring at least 8 in all five dimensions with no blockers (C41's acceptance stands on attempt-02; this attempt continues the push to 9). The owner's standing direction of 2026-10-30 ("go go go … until we are 100% complete") is the acceptance; the ledger entry is written when the P09 chunks are run through `xtask review`/`xtask complete`.

Accepted failure, to be remediated to a passing 9/10 before the P09 phase review (D034: a new generation, later chunks not reopened):

- R1: the response pane rendered only the first running transfer, so with two sends of one request in flight (Send, another tab, back, Send again) one progress region and one Cancel showed and only the first id could be cancelled; the core runs several sends of one document at once and the legacy editor listed every running entry. Expose progress and independent cancellation for every active run of the shown request, History inspected or not; add a regression with two deferred sends across a tab round trip, each cancelled by its own id exactly once, nothing resent.
