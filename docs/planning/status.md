# Current state

G00 and G01 passed (historical Claude reviews). C01–C04 and the P01 cumulative review completed on
2026-10-17; C05–C07 and the P02 cumulative review completed on 2026-10-18;
C08–C12 and the P03 cumulative review completed on 2026-10-19; C13–C15 and
the P04 cumulative review completed on 2026-10-20; C16–C18 and the P05
cumulative review completed on 2026-10-21; C19–C22 and the P06 cumulative
review completed on 2026-10-23; C23–C24 and the P07 cumulative review
completed on 2026-10-24; implementation is at **C25** (P08). D026 (2026-10-17) makes Codex the independent reviewer of every chunk
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
| C23 | **Complete** (generation 2): completed at 8/10 under D034, then raised; Codex attempt-05 passed; committed as ba9cd27 and verified | Follow-ups remediated (`reviews/C23/follow-ups.md`) |
| C24 | **Complete** (generation 2): completed at 8/10 under D034, then raised; Codex attempt-02 passed; committed as 582ae06 and verified | Follow-ups remediated (`reviews/C24/follow-ups.md`) |
| P07 | **Complete**: Codex phase attempt-04 scored 9/9/9/9/9; committed as 6a7253c2 and verified against the reviewed source fingerprint | Integrated native checks for C01–C24, every workload and the production footprint on the P07 bundle and tree, and 76 mutation probes; attempts remediated (`reviews/P07/remediation-attempt-0*.md`) |
| C25–C26 | Planned | Ordered predecessors required; C25 opens P08 |
| C27 | **Reviewed** (P09, D058): Codex attempt-05 scored 9 in all five dimensions through the fresh-exec fallback (`reviews/C27/attempt-05/receipt.json`); controller receipt and ledger completion regenerate once C26 completes | Attempts 01–04 remediated (`reviews/C27/remediation-attempt-0{1..4}.md`); follow-ups in `reviews/C27/follow-ups.md` |
| C28 | **Reviewed** (P09, D058): Codex attempt-09 scored 9 in all five dimensions through the fresh-exec fallback (`reviews/C28/attempt-09/receipt.json`); native acceptance C28-N01–N07 recorded on the prepared fixture bundle; controller receipt and ledger completion regenerate once C26 completes | Attempts 01–08 remediated (`reviews/C28/remediation-attempt-0{1..8}.md`); follow-up in `reviews/C28/follow-ups.md` |
| C29 | **Reviewed, live row blocked** (P09, D058): Codex attempt-06 scored 9/9/9/8/8 with no required fixes (`reviews/C29/attempt-06/receipt.json`); validation and overall stay at 8 only for the mandatory live Vulture verify receipt, which the approved manifest does not designate (`evidence/C29/live-scope-request.md` names the family to add); native acceptance C29-N01–N06 recorded on the prepared fixture bundle; controller receipt and ledger completion regenerate once C26 completes | Attempts 01–05 remediated (`reviews/C29/remediation-attempt-0{1..5}.md`) |
| C30 | **Reviewed, live row blocked** (P09, D058): Codex attempt-09 scored 9/9/9/8/8 with no required fixes (`reviews/C30/attempt-09/receipt.json`); validation and overall stay at 8 only for the predecessor C29's mandatory live Vulture verify receipt (B1/E1; `evidence/C29/live-scope-request.md` names the family to add); Inbox, reads, the tenant-switch guard and Home attention are core-owned per-tenant scopes; native C30-N01–N04 and the tenant-switch workload recorded on the prepared fixture bundle; attempts 01–08 remediated (`reviews/C30/remediation-attempt-0{1..8}.md`); controller receipt regenerates once C26 completes | Attempt history 7/6/6/6/6 → 8/7/8/7/7 → 8/7/8/8/7 → 9/8/8/8/8 → 9/8/9/8/8 → 9/8/9/8/8 → 9/9/9/8/8 → 9/8/9/8/8 → 9/9/9/8/8 |
| C31 | **Reviewed, live row blocked** (P09, D058): Administration renders the core's `AdminInventory` per tab; reviews apply only with the typed name through the native confirmation, and every review exit retires one-time keys in the core; Runtime key rotation is the core-owned staged sequence with its Inbox reminder, two stored timestamps and attempt-specific reconciliation; prompt and response topic results keyed apart from detectors; live topic review row passed against Redtail (evidence/C31/live) | Attempt-01 7/6/6/6/6, attempt-02 8/7/8/8/7, attempt-03 8/8/8/8/8, attempt-04 9/9/9/8/8 with no required fixes; reviewed, live row blocked (C29 B1/E1, owner-gated) |
| C32 | **Reviewed, live row blocked** (P09, D058): the Harness in the v2 shell on the existing engine, sessions, decisions, history, sign-in, project and MCP modules; read-only calls decided in the page; follow-ups only as the operator's review (proposals now include a key rotation's hand-off and the conversation workspace's guardrails); a fresh tenant's first send starts a conversation; lifecycle fixtures blocks, mcp and keys in the contracts package | Attempt-01 8/6/7/7/7, attempt-02 9/8/9/8/8, attempt-03 9/8/9/8/8, attempt-04 9/8/9/8/8, attempt-05 9/8/9/8/8, attempt-06 9/8/9/8/8, attempt-07 9/8/9/8/8, attempt-08 9/8/9/8/8, attempt-09 9/8/9/8/8, attempt-10 9/8/9/8/8, attempt-11 9/8/9/8/8, attempt-12 9/8/9/8/8, attempt-13 9/8/9/8/8, attempt-14 9/8/9/8/8, attempt-15 9/8/9/8/8, attempt-16 9/8/9/8/8, attempt-17 9/8/9/8/8, attempt-18 9/8/9/8/8, attempt-19 9/8/9/8/8, attempt-20 9/9/9/8/8 with no required fixes (fail solely for C29's owner-gated live Vulture receipt, B1/E1); manual receipt `reviews/C32/attempt-20/receipt.json` |
| C33 | **In review** (P09, D058): API Client and Test Cases in the v2 shell; Send's completion status region (known gap 5); Test Cases with search, product filter, pages of 20, pack names and recorded Playground results; Runtime cases opened in the Playground, every other product inspected only; "Show the calls" from the core's metadata-only call record | Attempt-01 9/7/8/7/7, attempt-02 9/7/9/7/7, both remediated; attempt-03 stopped. **Core committed without a receipt** under the P09 re-cut (`docs/planning/p09-recut.md`): the Test Cases and API Client screens here are superseded by C40 and C41 |
| C34 | Superseded by C44 (P09 re-cut, D059) | — |
| C35 | **Complete** (P09, D059): shell port — the R2App sidebar, header, tenant menu and palette over the existing core; the Classic switch lives in Settings only; parity composites approved by the owner (2026-10-29) | Attempt-01 9/7/9/7/7 and attempt-02 9/8/9/8/8 remediated (`reviews/C35/remediation-attempt-0{1,2}.md`); attempt-03 **9/9/9/9/9 pass**, no required fixes. Deferred optional notes: select the appearance before the N02 captures; keep check logs under attempt-specific paths (taken up in C36's runner) |
| C36 | **Complete** (P09, D059): Home — the R2App greeting row, six product cards from the core's read, Needs attention beside Quick actions; parity composites approved by the owner (2026-10-29) | Attempts 01 (9/7/9/7/7), 02 (9/9/9/8/8) and 03 (9/8/9/8/8) remediated (`reviews/C36/remediation-attempt-0{1,2,3}.md`); attempt-04 **9/9/9/9/9 pass**, no required fixes. Deferred optional note: measure the attention text container rather than the title's width in C36-N02 |
| C37 | Planned (P09, D059): Inbox fix-up | `docs/planning/chunks/C37.md` |
| C38 | **In progress** (P09, D059): Harness — the R2App title band, 244px conversation rail, transcript with the 46px composer and 350px Decisions aside over the C19–C33 behaviour; setup panels and texts as v2 components; parity composites captured by the native script, awaiting the owner's judgement | `docs/planning/chunks/C38.md`; `tools/native-acceptance/acceptance_c38.py`; `crates/desktop-core/tests/ui_v2_harness_port/` |
| C39 | **In progress** (P09, D059): Administration — C39a: the R2App product cards, tab strip, Runtime Security Overview, Playground and inventory table as v2 components; C39b: the other products and the review sheet | `docs/planning/chunks/C39.md`; `tools/native-acceptance/acceptance_c39.py`; `crates/desktop-core/tests/ui_v2_admin/` |
| C40 | **In progress** (P09, D059): Test Cases — the R2App title row, Explain banner, one table card with search, product pills, result pills, the action and ⋯ menu, the packs and new-case dialogs and the detail sheet over the C33 behaviour | `docs/planning/chunks/C40.md`; `tools/native-acceptance/acceptance_c40.py` |
| C41 | **In progress** (P09, D059): API Client — the R2App title row with the Collection and Environment chips, the 286px request tree, open-request tabs, the request line, the six request sub-tabs with the Params/Headers tables and the response pane with Body/Headers/Details/History over the C14/C15 behaviour | `docs/planning/chunks/C41.md`; `tools/native-acceptance/acceptance_c41.py`; `crates/desktop-core/tests/ui_v2_api/` |
| C42 | **In progress** (P09, D059): Academy — the R2App title row with the learner's pills, the 1.4fr/1fr frame (Academy's learner pages beside Your place) and the Learning paths row around the hosted learner pages with the Rust-owned theme | `docs/planning/chunks/C42.md`; `tools/native-acceptance/acceptance_c42.py`; `crates/desktop-core/tests/ui_v2_academy/` |
| C43 | **In progress** (P09, D059): Wizard — the R2Onboard presentation (320/244px rail with brand tile, numbered steps, tags, privacy note and appearance; pinned footer; section cards, 2-column grid, segmented control, switches; the add sheet at 1160×780) over the C29 state machine and intents | `docs/planning/chunks/C43.md`; `tools/native-acceptance/acceptance_c43.py`; `crates/desktop-core/tests/ui_v2_wizard/` |
| C44 | **In progress** (P09, D059): legacy shell removal and P09 closure — one presentation (the core always says v2, the switch intent is accepted and ignored, the Settings switch and the legacy shell, palette and feature views no v2 screen imports are gone, the product destinations open Administration), the drivers' wizard-based setup, the legacy-script map, the cumulative acceptance; the ledger pass and the phase review follow | `docs/planning/chunks/C44.md`; `tools/native-acceptance/acceptance_c44.py`; `crates/desktop-core/tests/ui_v2_closure/` |

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
