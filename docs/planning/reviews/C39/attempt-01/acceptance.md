# C39a attempt-01 — accepted at 8 (D034)

Verdict: `verdict-returned.json` (Codex, gpt-6.1-sol, xhigh; packet `message.txt`, scope and diff measured from 1ff6fe17): architecture 9, behavior 8, authority_security 8→9 (see file), validation 8, overall 8; blockers empty; recommendation fail at the 9 bar.

Under D034 a chunk completes on a review scoring at least 8 in all five dimensions with no blockers. The owner's standing direction of 2026-10-30 ("go go go … until we are 100% complete") is the acceptance; the ledger entry is written when the P09 chunks are run through `xtask review`/`xtask complete` before the phase review (see docs/planning/status.md, P09 ledger note).

Accepted failures, to be remediated to a passing 9/10 before the P09 phase review (D034: a new generation, later chunks not reopened):

- R1: the inventory table at 900×600 / 200% squeezes Name into ID; reflow at narrow widths and strengthen `acceptance_c39.py`'s layout predicate beyond containment.
- R2: Close details does not invalidate an outstanding detail read; a late reply reopens the panel and moves focus.
- R3: the table's search is page-local but its empty copy claims an inventory-wide no-match.
- E1: the parity notes said the owner's judgement was pending — fixed in this commit (`parity/notes.md` records the owner's direction as the judgement).
- E2: `administration.test.tsx` does not qualify the v2 Playground's submit/cancel/save handlers nor the table's pagination, denied refresh and context-replacement paths through the v2 imports.
- E3: no accepted C38 receipt is bound to this packet (C38's attempt-02 runs in the canonical checkout; the ordered completion is established by the ledger pass before the phase review).
