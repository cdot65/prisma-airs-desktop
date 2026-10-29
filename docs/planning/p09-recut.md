# P09 re-cut: port the prototype, one gate per screen

Agreed with the owner on 2026-10-29 after C32 took twenty review rounds while
the built v2 screens never matched the design. This supersedes the chunking
of C33–C34 in `IMPLEMENTATION-PLAN.md`; the specification
(`docs/architecture/ui-v2/`) and the core work of C27–C33 stand.

## The bar

- **The prototype is the spec.** `docs/architecture/ui-v2/prototype/` (Refined
  Native v2, version 134, checksums in `MANIFEST.sha256`) is the reference for
  every screen; the canvas is not consulted.
- **A screen is done when it matches its board.** Side by side at 1440×900 in
  light and dark, with real core data wired, and the fast checks (typecheck,
  Vitest, the affected Rust suites) passing. Parity is structural: regions,
  widths, spacing, type scale, tokens and component shapes. Content differs;
  accessibility corrections (focus rings, contrast, status regions) may deviate
  and are noted. The owner judges each composite.
- **Composites** live at `docs/planning/evidence/<chunk>/parity/<board>-<theme>.png`
  (prototype left, app right) with `notes.md` listing deviations, produced by
  `tools/parity/compose.py` (headless render of the prototype board beside the
  fixture app's own capture).
- **One heavy gate per chunk**, after the owner's parity approval: rebuild,
  native acceptance, the full Rust fixture suite and the Codex review, accepted
  at **8**. The phase review (after C44) is **9**; the C29 live Vulture row runs
  before it once the owner designates the L19 family.
- **No legacy views inside v2.** Each screen is new components over the
  existing core intents and `ShellSnapshot`; the legacy feature components stay
  untouched for the legacy shell until C44 removes it.
- **Cadence.** The shell is approved before any screen inside it. After that
  a chunk's composite is posted and work moves to the next chunk; the owner's
  notes are applied as they arrive.

## Chunks

| Chunk | Screen | Board(s) | Notes |
|---|---|---|---|
| C33 | core: case results and packs, call log, submissions | — | committed without a receipt; its two screens are superseded by C40/C41 |
| C35 | Shell | R2App any start | 56px header (title, 260px search, Explain switch = `showCalls`, Harness button); nav with product children and badges; status block; tenant menu 360px; palette 640px; tokens: text 13px, gaps, scrims, header/section/overlay sizes; Classic switch moves to Settings |
| C36 | Home | start=home | 3-column product cards with sparklines; Needs attention beside Quick actions |
| C37 | Inbox | start=inbox | fix-up: 430px list column, 3-column facts grid |
| C38 | Harness | start=harness (+ convo boards) | title band; 244px rail; transcript with 46px composer; 350px Decisions aside; own setup panels |
| C39a | Administration: Runtime Security | start=admin | section + tab strip; overview stat grid with sparkline; Playground form; generic tables over the management reads |
| C39b | Administration: other products | start=admin, start=review | the remaining products on the same pattern; the review sheet (620px) |
| C40 | Test Cases | start=cases | explain banner; one table card; packs and new-case dialogs (560px); detail sheet (520px) |
| C41 | API Client | start=api | full port: title row with chips; 286px request tree; open-request tabs; request sub-tabs; response pane |
| C42 | Academy | start=academy | prototype chrome around the hosted learner embed; composite judged on the chrome |
| C43 | Wizard | R2Onboard (all steps, mode=add) | re-port of the presentation over the existing state machine: pinned footer, 320/244px rail, section cards, segmented control, switch; model-driven extras housed in the prototype's cards |
| C44 | Legacy shell removal + phase review | — | on a branch; then the P09 review at 9 |

Each chunk brief under `docs/planning/chunks/` names the board, the regions
to port, the core intents and snapshot fields it reads, the native checks and
the test ids. The controller registers C35–C44 with a bar of 8.

## Fast loop

While building: `pnpm --filter @prisma-airs/desktop exec tsc --noEmit`,
`vitest run` for the desktop app, and `cargo test -p desktop-core --features
test-fixtures --test <suite>` for suites the change touches. The rebuild and
native run happen only at the gate, or when the composite needs a fresh capture.
Reviews run from a separate worktree so work continues meanwhile.

## First moves

1. Commit C33 as it stands (core, tests, the superseded screens).
2. Commit this re-cut: chunk briefs, controller registration, checks.
3. Build `tools/parity/compose.py` and run it on the current shell: the first
   composite is the baseline gap.
4. C35.
