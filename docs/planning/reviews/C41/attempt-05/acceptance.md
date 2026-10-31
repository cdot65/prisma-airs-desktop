# C41 attempt-05 — accepted at 8 (D034)

Verdict: `verdict-returned.json` (Codex, gpt-6.1-sol, xhigh; packet `message.txt`, scope and diff measured from C40's pre-gate commit 73048f76 at f1083fce): architecture 9, behavior 8, authority_security 9, validation 8, overall 8; blockers empty, evidence gaps empty, three required fixes. Attempt-04's fix (every running transfer listed) was remediated in f1083fce and is not raised again.

Under D034 a chunk completes on a review scoring at least 8 in all five dimensions with no blockers (C41's acceptance stands on attempt-02; this attempt continues the push to 9).

Accepted failures, to be remediated to a passing 9/10 before the P09 phase review (D034: a new generation, later chunks not reopened):

- R1: a document result was accepted without checking that its load was still current, so two selections in flight resolving out of order left the current selection without an editor. Obsolete results must not replace the current selection or resurrect closed documents or collections; regression with deferred results resolving out of order.
- R2: a selection was marked followed before its load succeeded and a refused or undelivered load was ignored, leaving a blank editor with no reason and no way to retry (selecting the tab again did not reopen it). Expose the reason and an explicit recovery action, no automatic retry; cover a refused result and a lost observation.
- R3: a tool's failure (an invalid cURL import, an environment or schema save) was shown only in the API Client's background alert, which the shell makes inert and hidden while the sheet is open. Present the failure inside the active sheet, keeping entered work and focus; regression asserting the error is exposed while the dialog stays open.
