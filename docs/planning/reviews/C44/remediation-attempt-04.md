# C44 attempt-04 remediation

Attempt-04 scored 9/8/9/8/8 with no blockers and was accepted at 8 under D034
(`attempt-04/acceptance.md`). Attempt-05 is the new generation for the push to 9;
its gate and review have not run yet.

| Finding | Remediation | Where |
|---|---|---|
| R1: a copied or cloned collection folder retains its collection id, allowing another folder's document to be reused | Scope documents and loads by the full folder plus collection id. The replacement regression covers a different id, the same id in another folder, and the same id and folder basename under another parent; each replacement loads the new document and ignores the old collection's late reply. | `417d93e4`; `apps/desktop/src/v2/workbench/api-client.tsx` and `api-client.test.tsx` |
| E1: reusing attempt-03's folder included packet outputs in its own source inventory and overwrote the prior verdict | Exclude the current attempt's review files from its source inventory. Advance past folders holding a verdict, with the overwritten attempt-03 acceptance also reserving its folder. Preserve the recovered attempt-04 artifacts separately and record the loss of attempt-03's original verdict; its scores and findings remain in its acceptance and remediation records. | `/Users/cdot/development/redtail/p09-queue/make-packet-c44.py` and `chain-c44-gate.sh`; `attempt-03/verdict-overwritten-note.md`; `attempt-03/acceptance.md`; `remediation-attempt-03.md`; `attempt-04/` |
| O1: an entry absent from the map could escape coverage checks | Compare the map's complete key set with the complete legacy native-check manifest before evaluating mapped targets. Missing and extra entries become uncovered assertions and fail C44-N01. | `b756e798`; `tools/native-acceptance/acceptance_c44.py` |

The handoff preflight runs without screen use. Fresh native acceptance and the
independent review remain required for attempt-05. The packet records that Claude
Code implemented the application changes through `b756e798`, while Codex prepared
and orchestrates attempt-05; a fresh Codex reviewer provides session separation
within the current implementer's model family.
