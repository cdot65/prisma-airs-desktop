# C44 attempt-04 remediation

Attempt-04 scored 9/8/9/8/8 with no blockers and was accepted at 8 under D034
(`attempt-04/acceptance.md`). Attempt-05 is the new generation for the push to 9;
its first native run was stopped during preflight remediation, before checks or
review. Fresh native acceptance and an independent verdict remain required.

| Finding | Remediation | Where |
|---|---|---|
| R1: a copied or cloned collection folder retains its collection id, allowing another folder's document to be reused | The first fix (`417d93e4`) combined the displayed folder and id, but the display contains only a basename. Attempt-05 preflight reproduced that remaining collision. Rust now exposes its existing opaque private-document scope, derived from the full root plus id; the renderer uses it for documents and loads. The three replacement cases use actual basenames and distinct core scopes. | `crates/desktop-core/src/authority/workbench_domain.rs`; `crates/desktop-core/tests/ui_v2_api/main.rs`; `apps/desktop/src/v2/workbench/api-client.tsx` and `api-client.test.tsx`; generated `CollectionView.ts`; contract parser and fixtures |
| E1: reusing attempt-03's folder included packet outputs in its own source inventory and overwrote the prior verdict | Exclude the current attempt's review files from its source inventory. Advance past folders holding a verdict, with the overwritten attempt-03 acceptance also reserving its folder. Preserve the recovered attempt-04 artifacts separately and record the loss of attempt-03's original verdict; its scores and findings remain in its acceptance and remediation records. | `/Users/cdot/development/redtail/p09-queue/make-packet-c44.py` and `chain-c44-gate.sh`; `attempt-03/verdict-overwritten-note.md`; `attempt-03/acceptance.md`; `remediation-attempt-03.md`; `attempt-04/` |
| O1: an entry absent from the map could escape coverage checks | Compare the map's complete key set with the complete legacy native-check manifest before evaluating mapped targets. Missing and extra entries become uncovered assertions and fail C44-N01. | `b756e798`; `tools/native-acceptance/acceptance_c44.py` |

## Attempt-05 preflight correction

The owner approved the host run. `c44chain11` started at 03:49 on 2026-11-01,
prepared executable `6ce22d608b7cc25ea7190c7e4d21b91e802e2bf737aac9da2c01e83328f244ed`,
and passed C11's 11 native checks. It was stopped during C10 after inspection
found that `CollectionView.folder` is produced by `folder_name(root)`, which
returns only the basename. No checks packet or verdict was produced. Preparation,
C11's record, C10's partial evidence and the job log are archived under
`/Users/cdot/development/redtail/p09-queue/codex-c44-preflight/aborted-native-20261101-0349/`.

The faithful renderer reproduction failed with two `openRequest` calls instead
of the expected three, while the three earlier path-shaped fixtures passed.
The production-authority regression opens two real `api` directories with the
same collection id and persisted request selection. Before the fix they had no
distinct scope in the view; afterwards their opaque scopes differ and reopening
the same root preserves its scope. The four API Client core tests pass. The
renderer regression also holds an old request reply across the replacement and
checks that it cannot replace the new document.

The shared `node_modules` symlink also resolved the contracts package from main
rather than the active checkout: a missing-scope parser case incorrectly passed
until this was exposed by the new validation assertion. Vite, Vitest and
TypeScript now resolve this checkout's contract parser, bindings and fixtures
explicitly. The parser requires the core's 32-character lowercase hexadecimal
scope; missing, path-shaped and malformed values fail admission.

This correction was implemented by Codex on an isolated branch so the owner's
uncommitted core file was not edited. Integration stashes only that named file
around the cherry-pick and restores it, with the original owner content checked
byte for byte after removing the three added scope lines. Claude Code implemented
the earlier application changes through `b756e798`; Codex implements this
preflight correction and orchestrates the gate. The fresh read-only Codex reviewer
provides session separation within the current implementer's model family.
