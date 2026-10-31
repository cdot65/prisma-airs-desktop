# C40 attempt-02 — accepted at 8 (D034)

Verdict: `verdict-returned.json` (Codex, gpt-6.1-sol, xhigh; packet `message.txt`, scope and diff measured from 49a5bac1): architecture 9, behavior 8, authority_security 9, validation 8, overall 8; blockers empty, evidence gaps empty; two required fixes; recommendation fail at the 9 bar.

Under D034 a chunk completes on a review scoring at least 8 in all five dimensions with no blockers. The owner's standing direction of 2026-10-30 ("go go go … until we are 100% complete") is the acceptance; the owner approved the composites the same day; the ledger entry waits on C26 (docs/planning/status.md, P09 ledger note). The owner's dependency amendment of 2026-10-30 applies.

Accepted failures, remediated for the push to 9 (attempt-03, a new generation):

- R1: `table_usable`'s readability regions were not bound to one case row (the recorded name was the hidden Action header, the input never measured, the result button not recognised); bind name, input, result and action to the same row, recognise the result button's label, reject missing, collapsed or overlapping regions.
- R2: a failed import write leaves the packs dialog open while the unsaved warning and Save again stay behind the inert page, and `notSaved` has no text; present the unsaved outcome, its reason and the recovery inside the open dialog.
