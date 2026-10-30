# C39b attempt-01 — accepted at 8 (D034)

Verdict: `verdict-returned.json` (Codex, gpt-6.1-sol, xhigh; packet `message.txt`, scope and diff measured from 640103e7): architecture 9, behavior 8, authority_security 9, validation 8, overall 8; blockers empty; four required fixes, two evidence gaps; recommendation fail at the 9 bar.

Under D034 a chunk completes on a review scoring at least 8 in all five dimensions with no blockers. The owner's standing direction of 2026-10-30 ("go go go … until we are 100% complete") is the acceptance; the ledger entry waits on C26 (docs/planning/status.md, P09 ledger note).

Accepted failures, remediated in the C39 generation-2 review on the c44-closure worktree (D034: a new generation at 9, later chunks not reopened):

- R1: Close review (X, Escape, the scrim) unmounts the Gateway editor without flushing a draft typed within the save delay; flush before removal, never applying.
- R2: the open sheet's draft note is captured when the sheet opens and never follows `gateway.local.problem`/`unsaved`; derive it from the current state, distinguishing pending, rejected and failed saves.
- R3: the Red Team source coverage (completeness, total, failure) is shown on Overview only; Scans, Targets, Attack results and Reports must carry it, including empty and denied reads.
- R4: the narrow-width table reflow hides the header row and leaves stacked values unlabeled; keep accessible column labels during reflow and make the native predicate assert them.
- E1: v2 regressions for R1–R3; C39-N03/N04 must assert their stated per-table and header requirements.
- E2: C38's acceptance record (`reviews/C38/attempt-02/acceptance.md` on main, accepted at 8 under D034) was not bound to this packet; the generation-2 packet names it. The owner's dependency amendment of 2026-10-30 applies.
