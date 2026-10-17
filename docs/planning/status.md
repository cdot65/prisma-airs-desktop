# Current state

G00 and G01 passed (historical Claude reviews). C01 and C02 completed on
2026-10-17; implementation is next at **C03**. D026 (2026-10-17) makes Codex the independent reviewer of every chunk
and of a cumulative review after every phase, each requiring at least 9 in all
five dimensions with empty required fixes, blockers and evidence gaps.

| Gate/chunk | State | Evidence/next action |
|---|---|---|
| G00 | Passed (historical) | Attempt03 scored 9 in all five dimensions |
| G01 | Passed (historical) | Attempt05 scored 9 in all five dimensions; fingerprint covers commit 931916b inputs only |
| C01 | **Complete** (generation 1): Codex attempt-04 scored 9 in all five dimensions; bootstrap agreement passed (52 adversarial cases including 16 typed, 2 controls); committed as ce5bd8d and verified against the reviewed source fingerprint | Attempts 01–03 failed and were remediated (records retained) |
| C02 | **Complete** (generation 1): Codex attempt-03 scored 9 in all five dimensions; committed as dfcf9bb and verified against the reviewed source fingerprint | Attempts 01–02 failed and were remediated (records retained) |
| C03–C26 | Planned | Ordered predecessors required; P01 phase review after C03 |

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
