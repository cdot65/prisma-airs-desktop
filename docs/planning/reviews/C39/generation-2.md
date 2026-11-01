# C39 generation 2 (D034): the Administration destination re-reviewed at 9 after its two chunks were accepted at 8

| Generation 1 | Verdict | Acceptance | Where |
|---|---|---|---|
| C39a (AI Runtime Security end to end, attempt-01 on c39-admin) | 9/8/9/8/8, fail at 9, no blockers | accepted at 8; R1–R3, E1–E3 | `reviews/C39/attempt-01/` (this tree: verdict-returned.json, acceptance.md) |
| C39b (the other products and the review sheet, attempt-01-c39b on c39b-admin) | 9/8/9/8/8, fail at 9, no blockers | accepted at 8; R1–R4, E1–E2 | `/Users/cdot/development/redtail/prisma-airs-desktop-v2-c39b/docs/planning/reviews/C39/attempt-01-c39b/` (verdict-returned.json, acceptance.md); merged into main by the merge chain |

Every accepted failure is remediated on this tree (c44-closure), which is where this generation runs:

- C39a R1: the inventory rows stack below 760 CSS px; the native predicate rejects overlapping texts (2327a774).
- C39a R2: Close details retires the read in flight and the opened item (2327a774).
- C39a R3: the page-local search's empty copy (2327a774).
- C39a E2: the v2 Playground handlers and the table's paging, denied refresh and context replacement are tested through the v2 imports (2327a774).
- C39b R1: Close review (X, Escape, the scrim) flushes the Gateway draft typed within the save pause before the workbench is removed; the workbench keeps the latest typed text apart from the textarea for its own cleanup (`runtime-changes.tsx`, `review-sheet.tsx`, d85ca0ed); a regression closes the sheet at once and finds the exact text saved.
- C39b R2: the open sheet derives the draft note from the current state on every render: pending (a save in flight), refused or unconfirmed (the save's reply), rejected (`gateway.local.problem`), not stored (`gateway.local.unsaved`), kept (`review-sheet.tsx`); the Gateway tabs no longer hand a captured element to the sheet; a regression walks every state.
- C39b R3: every Red Team tab drawn from the scan list (Scans, Targets, Attack results, Reports) carries the Scans source line (read in full, a partial or capped sample of the reported total, a sample of an unknown total, unavailable) (`observation-tabs.tsx`); a regression checks the partial and the denied read on each tab.
- C39b R4: the tables' header rows stay in the accessibility tree at every width (visually hidden once the rows stack) and every stacked cell shows its column's name (`data-label` on the inventory, Gateway and observation tables; `tokens.css`); `acceptance_c39.py` requires the headers in the AX tree and, at 200%, every stacked value prefixed by its column's name.
- C39b E1: the regressions above; C39-N03/N04 assert their stated requirements.
- C39b E2: C38's acceptance record is `reviews/C38/attempt-02/acceptance.md` on main (accepted at 8 under D034; attempt-03, the push to 9, is queued). The owner's dependency amendment of 2026-10-30 applies.

The composites were approved by the owner on 2026-10-30 ("C39 C40 C41 C43 composites approved"; `evidence/C39/parity/notes.md`).
