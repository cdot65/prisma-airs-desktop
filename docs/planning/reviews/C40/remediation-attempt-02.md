# C40 remediation after attempt-02 (9/8/9/8/8, no blockers: accepted at 8, two required fixes)

Attempt-02 passes the chunk bar (`attempt-02/acceptance.md`). Both required fixes are remediated here for the push to 9 (attempt-03, a new generation).

| Finding | Remediation | Where |
|---|---|---|
| R1 [P2] `table_usable`'s readability regions were not bound to one case row: the recorded name was the hidden Action header, the input never measured, the result button not recognised, so moved or collapsed regions still passed | The regions are bound to one row by the first action's label (the case name after "Open in Playground: " or "Inspect: "): the name is that text, the row is the tree between the name and the action, the result is the row's "<Verdict>: Show the result of <case>" button or its "Not run" text, the input the row's longest remaining text; each region must exist, be at least 24 CSS px wide and 8 tall, and none may overlap another. | `tools/native-acceptance/acceptance_c40.py` |
| R2 [P2] a failed import write left the packs dialog open while the unsaved warning and Save again stayed behind the inert page; `notSaved` had no text | The unsaved warning (with its reason and Save again) is part of the problems the open dialogs show; `notSaved` reads "The change was made in the library shown here but could not be saved on this Mac."; a regression pushes `unsaved: 'unavailable', problem: 'notSaved'` after Import… and finds both explanations and Save again inside the open dialog, the rows kept, the import notice not claiming durability. | `test-cases.tsx`, `v2/admin/copy.ts`, `workbench.test.tsx` |
| S the parity notes' narrow-window deviation | Updated: rows stack below 760 CSS px. | `parity/notes.md` |
| S the row menu's focus movement and dismissal | Not changed in this generation (optional). | — |

Commands: `npx tsc --noEmit` clean; `npx vitest run src/v2/workbench/workbench.test.tsx` passes. Attempt-03 (rebuild, native, checks, packet, Codex) runs through the job runner.
