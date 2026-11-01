# C44 attempt-03 — accepted at 8 (D034)

Verdict: `verdict-returned.json` (Codex, gpt-6.1-sol, xhigh; packet `message.txt`, scope and diff measured from 2acc5be4 with the predecessors' merges excluded as their reviewed work): architecture 9, behavior 8, authority_security 9, validation 8, overall 8; blockers empty, two required fixes, one evidence gap, one optional suggestion.

Under D034 a chunk completes on a review scoring at least 8 in all five dimensions with no blockers. The owner's standing direction of 2026-10-30 ("go go go … until we are 100% complete") is the acceptance: every accepted failure is remediated at once and the chunk re-gated for 9 (attempt-04, a new generation).

Accepted failures, remediated for the push to 9:

- R1: the API Client kept the previous collection's document when one collection was replaced directly by another selecting a request with the same id (the boundary effect cleared the document only when the collection became null). Clear the document, its notes and the followed selection at every collection boundary; a regression replaces collection A with B selecting the same request id, loads B, never shows A's fields under B and ignores A's late reply, beside the reopen and new-request regressions.
- R2 / E1: the map retired C30-N01 (the tenant-switch guard with a Harness decision pending, through the switcher, the palette and the Go menu) and C31-N03 (the staged Runtime key rotation: Issue additional key…, the host's reveal, the review exit, the reminder's resumption, the old key's removal) to unit and component suites, and the gate's retirement branch accepted any reason. Both are normative (invariants I1, I6, I7) and neither D059 nor D060 retires them: restore native, executable-bound coverage (the C30 and C31 drivers of the first v2 shell on the one presentation where they run, or facts added to the v2 drivers) and bind every other retirement to an explicit, owner-accepted retirement record that the gate checks by id.
- O: the stale descriptions (the record's limitations, parity/notes.md, the map's introductory table) are refreshed for D060 and the eight self-run legacy drivers.
