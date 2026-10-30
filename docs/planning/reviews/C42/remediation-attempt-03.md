# C42 remediation after attempt-03 (9/8/9/8/8: B1 predecessor record, R2 incomplete, R3 contract field)

| Finding | Remediation | Where |
|---|---|---|
| B1 C41's review record is absent | Structural: C41's first two native runs stopped on driver and layout faults (fixed, 65f141f4 and 251160ee); its gate runs ahead of C42's attempt-04 in the queue, and attempt-04's packet carries its verdict path. The ledger entry waits on C26 (status.md, P09 ledger note). | queue order |
| R2 `PROGRESS_MUTATIONS` omitted `desktop.attempt.submit`, so a passed desktop lab (which completes its unit on the hosted side) left the Learning paths row stale, including after an uncertain submission | `desktop.attempt.submit` is a progress mutation: when it settles with success or error the row rereads `paths.list` (on error as a readback; the runner, not the row, resubmits with the same submission id). | `v2/academy/academy.tsx` |
| R2 acceptance | A production-provider regression mounts the Academy on a lab unit, starts the lab, submits with the first receipt lost (the row rereads once; the count stays; nothing is replayed by the row), submits again (the runner's same submission id; the lab passes; the row shows 4 of 4), in one generation without remounting. jsdom gets the WebView's `matchMedia`. | `academy.test.tsx` ("…when a desktop lab is submitted…", attempt-03 R2) |
| R3 the card adapter read `description`; `paths.list` supplies `tagline`; both fixtures invented `description` | `pathRows` reads the contract's `tagline` as the card's summary; the component fixtures and the synthetic catalog's `paths.list` name `tagline`; the native driver asserts the catalog's tagline on the card (N01 `path_tagline`); the component suite asserts it through the production provider. | `academy.tsx`, `academy.test.tsx`, `academy_catalog.py`, `acceptance_c42.py` |
| S narrow-window reflow of the frame; S appearance-change integration | Not changed in this generation (optional). | — |

Commands: `npx tsc --noEmit` clean; `npx vitest run src/v2/academy` 11 passed. Attempt-04 (rebuild, native, checks, packet, Codex) runs through the queue after C41's gate.
