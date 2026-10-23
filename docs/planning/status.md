# Current state

G00 and G01 passed (historical Claude reviews). C01–C04 and the P01 cumulative review completed on
2026-10-17; C05–C07 and the P02 cumulative review completed on 2026-10-18;
C08–C12 and the P03 cumulative review completed on 2026-10-19; C13–C15 and
the P04 cumulative review completed on 2026-10-20; C16–C18 and the P05
cumulative review completed on 2026-10-21; C19–C22 and the P06 cumulative
review completed on 2026-10-23; implementation is at **C23** (P07). D026 (2026-10-17) makes Codex the independent reviewer of every chunk
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
| C08 | **Complete** (generation 2): completed at 8/10 under D034 (c07cc8b), then raised; Codex attempt-03 passed with 9 in all five dimensions; committed as ada9234 and verified | Follow-ups R1–R3/E1 remediated (`reviews/C08/follow-ups.md`) |
| C09 | **Complete** (generation 2): completed at 8/10 under D034 (aef8431), then raised; Codex attempt-03 passed with 9 in all five dimensions; committed as deb34c6 and verified | Follow-ups remediated, including native sheet retirement and reveal ownership (`reviews/C09/follow-ups.md`) |
| C10 | **Complete** (generation 2): completed at 8/10 under D034 (90dbc40), then raised; Codex attempt-03 passed with 9 in all five dimensions; committed as a38b4f8 and verified | Follow-ups remediated, including production NSOpenPanel evidence (`reviews/C10/follow-ups.md`) |
| C11 | **Complete** (generation 2): completed at 8/10 under D034 (0e4bda6), then raised; Codex attempt-03 passed with 9 in all five dimensions; committed as aec2cbc and verified | Follow-ups remediated, including production save panel evidence (`reviews/C11/follow-ups.md`) |
| C12 | **Complete** (generation 2): completed at 8/10 under D034 (1911da6), then raised; Codex attempt-04 passed with 9 in all five dimensions; committed as 962747b and verified | Attempts 02–03 remediated: status stamps for library outcomes, hardened mutation harness, C12-N11 offline Library (`reviews/C12/follow-ups.md`) |
| P03 | **Complete**: Codex phase attempt-02 scored 9 in all five dimensions; committed as bbbad43 and verified against the reviewed source fingerprint | Attempt-01 R1 (context-bound Playground handoff) remediated (`reviews/P03/follow-ups.md`); integrated native checks for C08–C12, the C11 workload and 129/129 mutation probes on the P03 bundle |
| C13 | **Complete** (generation 2): completed at 8/10 under D034 (0d105e0), then raised; Codex attempt-03 passed with 9 in all five dimensions; committed as bdd57c2 and verified | Follow-ups R1–R4/E1 remediated, including nested rated targets, pagination shape and capped totals (`reviews/C13/follow-ups.md`) |
| C14 | **Complete** (generation 2): completed at 8/10 under D034 (8d219b0), then raised; Codex attempt-06 passed with 9 in all five dimensions; committed as 0c6a604 and verified | Follow-ups R1/R5/R6 remediated: body ownership by the file reached and by schema role, unchanged saves superseding drafts, bounded discovery (`reviews/C14/follow-ups.md`) |
| C15 | **Complete** (generation 2): completed at 8/10 under D034 (6d78143), then raised; Codex attempt-04 passed with 9 in all five dimensions; committed as e9df1db and verified | Follow-ups R2/R5 remediated: the effective credential set (including the jar Cookie) at registered origins and reviews bound to the credentials sent (`reviews/C15/follow-ups.md`) |
| P04 | **Complete**: Codex phase attempt-03 scored 9 in all five dimensions; committed as 8d5d436 and verified against the reviewed source fingerprint | Integrated native checks for C13–C15, the C13 and C15 workloads and 149/149 mutation probes on the P04 bundle |
| C16 | **Complete** (generation 2): completed at 8/10 under D034 (21266fe), then raised; Codex attempt-03 passed with 9 in all five dimensions; committed as 1811d43 and verified | Follow-ups R1/R2 remediated: models bound to the integration shown and the chosen workspace's detail readable, with C16-N10/N11 (`reviews/C16/follow-ups.md`) |
| C17 | **Complete** (generation 1): Codex attempt-03 scored 9 in all five dimensions; committed as 250a8ce and verified against the reviewed source fingerprint | Attempts 01–02 remediated (`reviews/C17/remediation-attempt-0{1,2}.md`); D043 recorded |
| C18 | **Complete** (generation 1): Codex attempt-04 scored 9 in all five dimensions; committed as 9b5f2ac and verified against the reviewed source fingerprint | Attempts 01–03 remediated (`reviews/C18/remediation-attempt-0{1..3}.md`); D044 and D045 recorded; T20 workload registered |
| P05 | **Complete**: Codex phase attempt-01 scored 9 in all five dimensions; committed as 5ce089b and verified against the reviewed source fingerprint | Integrated native checks for C16–C18, the C18 workload and 133/133 mutation probes on the P05 bundle |
| C19 | **Complete** (generation 1): Codex attempt-06 scored 9 in all five dimensions; committed as 7fee9f9 and verified | D046 recorded; Harness engine startup and memory workloads registered |
| C20 | **Complete** (generation 2): completed at 8/10 under D034, then raised; Codex attempt-03 passed; committed as aa4d4cf and verified | Follow-ups remediated (`reviews/C20/follow-ups.md`) |
| C21 | **Complete** (generation 2): completed at 8/10 under D034, then raised; Codex attempt-03 passed; committed as 702cc54 and verified | Follow-ups remediated (`reviews/C21/follow-ups.md`) |
| C22 | **Complete** (generation 2): completed at 8/10 under D034, then raised; Codex attempt-04 passed; committed as 8985a00 and verified | Follow-ups remediated (`reviews/C22/follow-ups.md`) |
| P06 | **Complete**: Codex phase attempt-05 scored 9/9/9/9/9; committed as 3dbb2ce and verified against the reviewed source fingerprint | Integrated native checks and workloads for C19–C22 and 182 mutation probes on the P06 bundle; attempts remediated (`reviews/P06/remediation-attempt-0*.md`) |
| C23–C26 | Planned | Ordered predecessors required; C23 opens P07 |

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
