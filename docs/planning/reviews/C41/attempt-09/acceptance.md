# C41 attempt-09 — accepted at 8 (D034)

Verdict: `verdict-returned.json` (Codex, gpt-6.1-sol, xhigh; packet `message.txt`, scope and diff measured from C40's pre-gate commit 73048f76 at ed38a9bf): architecture 9, behavior 8, authority_security 9, validation 8, overall 8; blockers empty, evidence gaps empty, two required fixes. Attempt-08's two fixes were remediated in ed38a9bf and are not raised again.

Under D034 a chunk completes on a review scoring at least 8 in all five dimensions with no blockers (C41's acceptance stands on attempt-02; this attempt continues the push to 9).

Accepted failures, to be remediated to a passing 9/10 before the P09 phase review (D034: a new generation, later chunks not reopened):

- R1: returning to a request whose editor had unmounted (another selection whose load stayed pending or failed) remounted the retained DocumentView, while the editor had owned newer protected fields: the URL reverted and a Save could have removed the protected draft. Reload the authoritative document when returning after the editor unmounts; regressions with A edited and protected, B pending then refused, the return to A, explicit Save receiving the current fields, no automatic send.
- R2: an environment's or schema's open reply replaced the editor unconditionally, even after a newer open, a new resource or Close; a draft's unsaved typing was lost. Bind opens to an editor generation retired by New, Close or another selection; regressions for a superseded schema open and environment open.
