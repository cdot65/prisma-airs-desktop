# C41 attempt-06 — accepted at 8 (D034)

Verdict: `verdict-returned.json` (Codex, gpt-6.1-sol, xhigh; packet `message.txt`, scope and diff measured from C40's pre-gate commit 73048f76 at 3d1827bf): architecture 9, behavior 8, authority_security 9, validation 8, overall 8; blockers empty, evidence gaps empty, three required fixes. Attempt-05's three fixes were remediated in 3d1827bf and are not raised again.

Under D034 a chunk completes on a review scoring at least 8 in all five dimensions with no blockers (C41's acceptance stands on attempt-02; this attempt continues the push to 9).

Accepted failures, to be remediated to a passing 9/10 before the P09 phase review (D034: a new generation, later chunks not reopened):

- R1: a tree click's document reply bypassed the load generations, and `show` read the collection at reply time, so a late tree reply replaced the current selection's document. Bind every document-producing reply to its initiating collection and load generation; regressions for a deferred tree click and a collection change.
- R2: the collection boundary invalidated load generations but left the loading marker, so a collection closed and re-opened while its selected request's first load was pending never loaded that request again (one openRequest, a blank editor, no alert). Retire loading markers at collection boundaries and load the re-opened selection afresh; late replies from the previous lifetime stay ignored.
- R3: the New request / New folder dialog received only a boolean, so a refused or undelivered creation (a duplicate path) was said only in the inert page behind it. Say the failure inside the dialog, keeping the path and focus, no automatic retry; regressions for a request and a folder.
