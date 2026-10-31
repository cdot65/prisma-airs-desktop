# C41 attempt-08 — accepted at 8 (D034)

Verdict: `verdict-returned.json` (Codex, gpt-6.1-sol, xhigh; packet `message.txt`, scope and diff measured from C40's pre-gate commit 73048f76 at 4db4508e): architecture 9, behavior 8, authority_security 9, validation 8, overall 8; blockers empty, evidence gaps empty, two required fixes. Attempt-07's two fixes were remediated in 4db4508e and are not raised again.

Under D034 a chunk completes on a review scoring at least 8 in all five dimensions with no blockers (C41's acceptance stands on attempt-02; this attempt continues the push to 9).

Accepted failures, to be remediated to a passing 9/10 before the P09 phase review (D034: a new generation, later chunks not reopened):

- R1: an environment's or a schema's save reply replaced the editor unconditionally, so typing made while the save was pending was lost. Bind the reply to its resource and the text it was sent: the new revision always, the text only when nothing newer was typed; the next Save sends the newer text against the new revision. Regressions for a private environment and a schema with a deferred save.
- R2: a cURL import's conversion report was discarded when the snapshot selecting its draft arrived before the import's result (the selection's own load superseded the import). Keep the current import's report for its draft without restoring obsolete documents; regressions for both orderings, later selections staying protected.
