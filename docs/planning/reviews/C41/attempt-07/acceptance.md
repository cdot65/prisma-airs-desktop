# C41 attempt-07 — accepted at 8 (D034)

Verdict: `verdict-returned.json` (Codex, gpt-6.1-sol, xhigh; packet `message.txt`, scope and diff measured from C40's pre-gate commit 73048f76 at 460778b1): architecture 9, behavior 8, authority_security 9, validation 8, overall 8; blockers empty, evidence gaps empty, two required fixes. Attempt-06's three fixes were remediated in 460778b1 and are not raised again.

Under D034 a chunk completes on a review scoring at least 8 in all five dimensions with no blockers (C41's acceptance stands on attempt-02; this attempt continues the push to 9).

Accepted failures, to be remediated to a passing 9/10 before the P09 phase review (D034: a new generation, later chunks not reopened):

- R1: a cURL import's load began only when its reply arrived (the callback outliving the sheet), so the reply took the current collection and a fresh generation instead of its initiating lifetime: an import deferred past a selection change replaced the current request's document. Capture the import's collection and generation before dispatch, retire it when superseded, ignore obsolete replies; regressions for a selection change and a collection close/re-open while the import is pending.
- R2: a tree entry's failed load was recorded by request id but shown only when that request was selected; the core leaves the previous selection unchanged when the read fails, so a refused or lost tree open showed nothing and offered no retry. Say the attempted request's failure in the current collection even with the selection unchanged, keep the previous editor, offer an explicit retry; cover a refusal and a lost observation without a fabricated selection.
