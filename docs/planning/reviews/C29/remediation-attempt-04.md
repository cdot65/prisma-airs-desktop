# C29 remediation after attempt-04 (9/8/9/7/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 Cancel stalled when the draft snapshot preceded the creation receipt | The pending creation is mirrored in state; its settlement re-runs the cancel decision whatever the order of receipt and snapshot. Tests: wizard.test (snapshot before receipt; receipt before snapshot) — one discard, close after admission | `wizard.tsx` |
| R2 an unconfirmed discard left no recovery | The unconfirmed status offers Retry (and a close request while unconfirmed is that retry), sending the discard again for the current draft; a draft seen gone still closes the sheet. Tests: wizard.test (draft retained → Retry discards and closes; draft gone → closes) | `wizard.tsx` |
| E2 flake logs from earlier input generations | `run-checks.sh` removes earlier runs' flake logs before a run, so any retained flake log is a same-input retry of the recorded run; the two historical logs were removed | `evidence/C29/runs/run-checks.sh` |
| Optional: the minimum-size capture's theme | Light is selected explicitly before the 900×600 capture and Match System restored after | `acceptance_c29.py` |

B1/E1 (the live Vulture verification receipt) stay blocked pending the owner's manifest family (`evidence/C29/live-scope-request.md`); synthetic acceptance does not close that row and the packet says so.
