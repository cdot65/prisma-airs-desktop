# Current state

G00 and G01 passed (historical Claude reviews). C01–C04 and the P01 cumulative review completed on
2026-10-17; C05–C07 and the P02 cumulative review completed on 2026-10-18;
implementation is at **C10** (P03). D026 (2026-10-17) makes Codex the independent reviewer of every chunk
and of a cumulative review after every phase, each requiring at least 9 in all
five dimensions with empty required fixes, blockers and evidence gaps. D034 (2026-10-18, owner rule) lets a chunk complete at 8/10 with no
blockers; each phase review still needs 9/10, after every chunk accepted below
9 is raised to a passing review.

| Gate/chunk | State | Evidence/next action |
|---|---|---|
| G00 | Passed (historical) | Attempt03 scored 9 in all five dimensions |
| G01 | Passed (historical) | Attempt05 scored 9 in all five dimensions; fingerprint covers commit 931916b inputs only |
| C01 | **Complete** (generation 1): Codex attempt-04 scored 9 in all five dimensions; bootstrap agreement passed (52 adversarial cases including 16 typed, 2 controls); committed as ce5bd8d and verified against the reviewed source fingerprint | Attempts 01–03 failed and were remediated (records retained) |
| C02 | **Complete** (generation 1): Codex attempt-03 scored 9 in all five dimensions; committed as dfcf9bb and verified against the reviewed source fingerprint | Attempts 01–02 failed and were remediated (records retained) |
| C03 | **Complete** (generation 1): Codex attempt-02 scored 9 in all five dimensions; committed as 1b69852 and verified against the reviewed source fingerprint | Attempt-01 (9/8/8/8/8) remediated (`reviews/C03/remediation-attempt-01.md`); budgets v3 acknowledged; C24 owns the typed-input memory observation |
| P01 | **Complete**: Codex phase attempt-02 scored 9 in all five dimensions; committed as 80d6c51 and verified against the reviewed source fingerprint | Attempt-01 (9/8/9/8/8) remediated (`reviews/P01/remediation-attempt-01.md`); 38/38 integrated native checks; budgets re-measured on the P01 bundle |
| C04 | **Complete** (generation 1): Codex attempt-03 scored 9 in all five dimensions; committed as 4c13792 and verified against the reviewed source fingerprint | Attempts 01 (8/7/7/8/7) and 02 (9/8/9/8/8) remediated (`reviews/C04/remediation-attempt-0{1,2}.md`); D028 recorded |
| C05 | **Complete** (generation 1): Codex attempt-03 passed; committed as 7afdd9b and verified against the reviewed source fingerprint | Attempts 01–02 remediated (`reviews/C05/remediation-attempt-0{1,2}.md`) |
| C06 | **Complete** (generation 1): Codex attempt-05 passed; committed as 8718ac9 and verified against the reviewed source fingerprint | Attempts 01–04 remediated (records retained); D030 recorded |
| C07 | **Complete** (generation 1): Codex attempt-06 scored 9 in all five dimensions; committed as 960e340 and verified against the reviewed source fingerprint | Attempts 01–05 remediated (`reviews/C07/remediation-attempt-0{1..5}.md`); D031 and D032 recorded |
| P02 | **Complete**: Codex phase attempt-01 scored 9 in all five dimensions; committed as 3decccc and verified against the reviewed source fingerprint | Integrated native checks for C04–C07 and the workload budget on the P02 bundle |
| C08 | **Complete** (generation 1) under D034: Codex attempt-01 scored 8/8/8/8/8; committed as c07cc8b and verified | Follow-ups R1–R3/E1 remediated in the tree (`reviews/C08/follow-ups.md`); raised to 9/10 before the P03 review |
| C09 | **Complete** (generation 1) under D034: Codex attempt-01 scored 8/8/8/8/8; committed as aef8431 and verified | Follow-ups R1–R3/E1 and the core part of E2 remediated in the tree (`reviews/C09/follow-ups.md`); raised to 9/10 before the P03 review |
| C10 | **In progress**: DLP authoring subset and staged keyword files implemented; native acceptance, probes and the Codex review next | D036 recorded; evidence in `evidence/C10` |
| C11–C26 | Planned | Ordered predecessors required; P03 phase review after C12 |

## C01 evidence (docs/planning/evidence/C01)

- Controller runs (`runs/`): `xtask check` (20 commands: install, bindings,
  workflow ledger, notice inventory, contract/desktop typecheck, desktop Vitest,
  production build, rustfmt, clippy in default and fixture feature sets, and the
  Academy shared-source regressions: scratch install with no lifecycle scripts,
  typecheck, package typecheck, 207 presentation tests, static web build);
  `xtask test` (shell_controller 42, ipc_admission 10, permission_manifest 5,
  frontend 18 tests); `xtask native-prepare` fixture bundle build. Failed
  attempts are retained alongside passing runs.
- `native-prepare.json`: fixture bundle identity and executable hash.
- `native/startup-network.json`: no internet sockets from the app or its
  WebKit processes over 15 s of startup (sampling evidence, limits recorded).
- `mutation-probes/`: 15 deliberate violations (review threshold, verdict
  hashing, attempt expiry, plan coverage, final reviewer output, native phase
  members, bundle binding, web scratch drift, delivery duplicates, raw-body
  ceiling, observation cap, focus scrolling, theme applied after paint,
  sequence reuse, window admission) each make their guard test fail;
  controls and the unmutated baseline pass.
- `native-failed/`: superseded native runs with their causes (radio-group
  driver defect; focus not scrolled into view at 200% zoom; rail destination
  under the bottom fade at 200% zoom; a passing run superseded when the theme
  moved into the snapshot's commit).
- `toolchain-correction.json`: Rust 1.90.0 → 1.99.0 with the dyld test matrix.
- `native-acceptance.json` + `native/`: all 13 C01 checks driven on the real
  fixture window through macOS Accessibility with fixture WebView self-capture
  (focus ring, themes, zoom, minimum size, Reduce Motion, menus, palette).
- `clean-checkout/`: fresh Academy clone from origin and fresh v2 tree (same
  origin configuration) pass check and test with the reviewed fingerprints;
  raw run records, logs and the reproduction script are retained.
- Review transport: preferred thread handshake confirmed
  (`reviews/transport/hs-20261017T000308Z-2eb173d5/route-ready.json`).

Shared Academy input: branch `v2/desktop-presentation` at
891f65eef76b53eb87155d0f0f5622228e217a2d, pushed to the Academy origin
(owner-approved; main untouched). No product endpoint has been called, no user data written, and no
signing, live or installed-package qualification has run.

This state file, `reviews/` and `evidence/` are the narrow mutable exclusions
from the source fingerprint; evidence is hashed separately per subject.
