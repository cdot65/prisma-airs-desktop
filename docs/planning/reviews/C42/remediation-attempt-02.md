# C42 remediation after attempt-02 (9/8/9/8/8: B1 predecessor record, R2 partial, E2 owner's judgement)

| Finding | Remediation | Where |
|---|---|---|
| B1 C41 has no review record in the supplied checkout | Structural: C41's gate runs ahead of C42's attempt-03 in the queue; attempt-03's packet carries C41's verdict path. The ledger entry itself waits on C26 (status.md, P09 ledger note). | queue order |
| R2 the Learning paths row shared `paths.list`'s key (invalidated by reset/enrollment) but a unit completion or a knowledge-check submission never invalidates that key, so the row's counts stayed stale while Academy's unit/course rollups refreshed | `PathsRow` subscribes to the generation's mutation cache: when a mutation keyed `units.complete` or `units.submitKnowledgeCheck` (`PROGRESS_MUTATIONS`) settles with success or error, `paths.list` is invalidated and reread — on success for the new counts, on an error (an uncertain outcome included) to read Academy's truth back without replaying the mutation. Academy's hooks are untouched. | `v2/academy/academy.tsx` |
| R2 acceptance | A production-provider regression renders the Academy on a unit, completes it with the first receipt lost (the row rereads once, the completion is not replayed, the count stays), completes it (the row shows 2 of 4), then starts, answers and submits the knowledge check that follows (3 of 4), all in one generation without remounting the Academy. | `academy.test.tsx` ("rereads the Learning paths row…", attempt-02 R2) |
| E2 the parity notes' judgement is the reviewer's own and the owner's standing direction; the reviewer requires the owner's explicit judgement of these composites | The owner was shown `parity/academy-{light,dark}.png` with the deviations listed and approved them on 2026-10-30 ("C42 composites approved"); `parity/notes.md` records the approval. | `parity/notes.md` |
| S run-checks.sh labelled ui_v2_workbench as C42's fixture command | Corrected to ui_v2_academy. | `evidence/C42/runs/run-checks.sh` |
| S the shell-mount test's title claimed theme coverage | Retitled; a unit test pins `themeFor` (dark→gruvbox, light→paper, system follows the system). | `academy.test.tsx` |
| S stack the frame at narrow effective widths | Not changed in this generation (the frame keeps two columns; the prototype is fixed-width). | — |

Commands: `npx tsc --noEmit` clean; `npx vitest run src/v2/academy` 10 passed. Attempt-03 (rebuild, native, checks, packet, Codex) runs through the queue after C41's gate.
