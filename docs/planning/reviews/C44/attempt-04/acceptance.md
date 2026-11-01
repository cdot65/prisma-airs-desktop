# C44 attempt-04 — accepted at 8 (D034)

Verdict: `verdict-returned.json` (Codex, gpt-6.1-sol, xhigh; request id C44-attempt-03-1793501336 and attempt_id "attempt-03" because the chain wrote this run into attempt-03's folder — see `../attempt-03/verdict-overwritten-note.md`; packet `message.txt`, scope and diff measured from 2acc5be4 with the predecessors' merges excluded): architecture 9, behavior 8, authority_security 9, validation 8, overall 8; blockers empty, one required fix, one evidence gap, one optional suggestion. Attempt-03's R2/E1 (the retired native workflows) are answered: the reviewer raises neither again.

Under D034 a chunk completes on a review scoring at least 8 in all five dimensions with no blockers. The owner's standing direction of 2026-10-30 ("go go go … until we are 100% complete") is the acceptance: every accepted failure is remediated at once and the chunk re-gated for 9 (attempt-05, a new generation).

Accepted failures, remediated for the push to 9:

- R1: the API Client scoped documents and loads by the collection id alone; a copied or cloned folder keeps that id, so another folder's request could show under the current collection. The scope is the folder and the id together (as the core's private-document scope is); the replacement regression now covers a changed id, the same id in another folder, and the same id and folder basename under another parent.
- E1: the packet's source inventory hashed the attempt folder's own earlier files (the chain reused attempt-03's folder) and so could not be reproduced. The packet excludes the attempt's own review artefacts from the attested scope, and the chain never writes into a folder that already holds a verdict.
- O1: the gate compares the map's key set with the legacy manifest before judging coverage, so a future unmapped check fails rather than being skipped.
