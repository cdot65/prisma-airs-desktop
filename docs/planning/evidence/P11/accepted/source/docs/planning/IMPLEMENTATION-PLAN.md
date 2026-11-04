# Prisma AIRS Desktop implementation plan

Current work: owner-approved [P11 refinements](refinements/2026-11-04/plan.md),
C46–C49, after accepted P10. The original ordered migration plan below remains
as its historical design record. C26 stable installed acceptance is still open;
the published v1.0.0-rc.1 is immutable and remains a prerelease.

Status: product interview decisions D001–D023 accepted; D024/D025 settle source
terms and all four architecture candidates; D026 makes Codex the independent
chunk and phase reviewer. G00/G01 passed; implementation proceeds from C01. A passing plan review is not a passing architecture,
SDK, application, live-tenant, or package review.

## Outcome and exclusions

Deliver a fresh macOS Apple Silicon desktop application with React presentation
in Tauri, a focused Rust application core, direct Rust SCM SDK integration, and
the native Rust Harness engine. Academy supplies shared presentation and remains
the authority for remote learning. Preserve agreed working v1 behavior, using
v1 as requirements reference. Do not translate its GPUI implementation wholesale.

No desktop billing, media-generation backend, local MySQL/Redis server stack,
Node/Bun backend runtime, TypeScript workers, speculative browser client, or
in-app updater is included. Academy gamification remains inside Academy. Gateway
usage/cost/budget/limits remain operational product functionality. Build-time
JavaScript tooling and React WebView JavaScript are expected.

Source destination: `ssh://git@git-ssh.cdot.io/cdot/prisma-airs-desktop-v2.git`.
Checkout: `/Users/cdot/development/redtail/prisma-airs-desktop-v2`.

## Architectural guiding principles

1. Design deep modules: substantial behavior behind a small domain interface.
   Apply the deletion test; do not wrap every SDK method in a pass-through module.
2. The interface is the test surface. Exercise production transitions and
   adapters; use independent wire/protocol expectations for external contracts.
3. One adapter is a hypothetical seam; two real adapters justify a seam. Shared
   Academy presentation has real web/desktop consumers. Do not build speculative
   browser/backend/plugin architectures for this desktop delivery.
4. Rust owns authority and locality for tenant/credential/workspace context,
   request lifecycle, configuration review, protected storage, and approvals.
   React owns presentation and editable input, not permission to execute.
5. The SDK owns its declared transport/authentication contracts. The application
   owns interactive identity, OS storage, context admission, and workflow policy.
   Do not duplicate SDK retry/pagination/OAuth policy without a documented need.
6. Unknown outcomes and partial/stale/denied/unsupported data remain distinct.
   Never map an unknown verdict to allow or a cancelled write to successful rollback.
7. Host credentials and capabilities are explicit. No ambient environment-file
   discovery, inherited production fallback, generic renderer secret access,
   unrestricted native plugin grants, or cross-origin credential forwarding.
8. Shared appearance does not imply shared learner state. Tenant operations,
   Academy identity/progression, Harness inference identity, and MCP grants stay
   separately scoped. A tenant switch does not sign the learner out of Academy.
9. Keep provenance and reproducibility local to dependency selection. Exact
   versions, source hashes, build inputs, notices, and protocol pins are evidence;
   a neighboring active worktree is not a release dependency.
10. Measure native usability/performance and the actual installed package. A
    successful browser render or mocked IPC assertion is insufficient evidence
    for Tauri host behavior, VoiceOver, Keychain, sidecar isolation, or notarization.

Concrete interfaces, crate decomposition, shared package extraction, and storage
formats are outputs of G01. This plan defines behavior and testable work, not
unreviewed Rust signatures or a predetermined hierarchy of tiny modules.

## Ordered phases and chunks

Every Cxx has its own instruction file under [chunks](chunks/README.md). Complete
chunks in numeric order. A dependency delay blocks its chunk; report it honestly
and continue only research/preparation explicitly within that chunk. Do not
silently advance a later phase or replace a real dependency with a fake to pass.
For a prolonged upstream stall, report the exact dependency and offer an owner
reorder/scope decision. Do not wait indefinitely or reorder silently; an approved
change updates predecessors, plan and review evidence before execution.

| Phase | Chunks   | Result                                                           |
|-------|----------|------------------------------------------------------------------|
| P00   | G00, G01 | Confirmed plan; selected and reviewed architecture               |
| P01   | C01–C03  | Shared desktop shell, native core, first complete Runtime scan   |
| P02   | C04–C07  | Academy identity, learner presentation, assessment, offline work |
| P03   | C08–C12  | Runtime/DLP administration, Sessions, reports, Library           |
| P04   | C13–C15  | Existing Red Team/Model reads and complete API Client workflows  |
| P05   | C16–C18  | Gateway workspace/configuration, reviewed changes, inference     |
| P06   | C19–C22  | Native Harness, Rust sign-in, typed host tools, workspace MCP    |
| P07   | C23–C24  | Reviewed imports, native quality, accessibility/privacy closure  |
| P08   | C25–C26  | Controlled live qualification; signed installed DMG acceptance   |
| P09   | C27–C44  | UI v2 "Refined Native": spec, shell, wizard, destinations (D058); re-cut from C35 as one screen per chunk ported from the prototype (D059) |
| P10   | C45      | Fresh application identity and one presentation (D061) |
| P11   | C46–C49  | Owner refinements: connection diagnostics, shell, API Client and Settings/themes (D063) |

### G00 — Plan confirmation

Finish the decision ledger, baseline coverage, chunk instructions, review/test
protocols, dependency gates, and architecture handoff. Obtain an actual independent
plan review meeting D019. Product decisions are accepted and the owner explicitly
selected the architecture candidates and source terms in D024/D025. Continue the
requested design work; no further blanket scope question is needed. Architecture
review and implementation evidence remain distinct gates.

### G01 — Architecture improvement workflow

Follow [the handoff](architecture-handoff.md) and the requested
improve-codebase-architecture skill. Explore the named areas, show a temporary
HTML report with before/after candidates, ask the owner to choose, and grill the
selected design. Produce ADRs, interface/test ownership, dependency/import
decisions, and concrete chunk command specifications. Review the architecture
at least 9/10 in every critical dimension and overall. Revise this plan if the
selected design affects it, then re-review affected planning evidence.

No application scaffolding, shared-source refactoring, real-data migration,
product endpoint calls, or packaging implementation begins before G01 passes.

## Per-chunk execution contract

1. Read AGENTS/CLAUDE, glossary, accepted ADRs, the chunk, predecessor receipts,
   dependency manifest, test catalogue, and its canonical implementer prompt.
2. Confirm a clean approved source snapshot and identify unrelated user work.
   Resolve dependency availability/provenance and baseline check limitations.
3. Implement only that chunk and approved supporting changes. A shared Academy,
   SDK, or Harness change needs its own reviewed revision and regression evidence;
   preserve active work and do not move another project's main branch casually.
4. Run exact checks from the approved command manifest. Use deterministic local
   fixtures and literal external contracts, then the appropriate native tests.
   Never call hosted application endpoints by accident.
5. Record commands, environments, inputs, results, failures, hashes, limitations,
   and rollback rehearsal. Save raw sensitive evidence privately; commit redacted
   summaries with references/hashes, not credentials or customer bodies.
6. Freeze source and dependency inputs. Request actual independent Codex
   review through [the independent prompt](prompts/review-chunk.md) (D026).
7. If any critical dimension/overall score is below 9, required fixes remain,
   source changed, or mandatory evidence is absent: remediate, rerun affected
   checks, freeze a new snapshot, and request a new review of the same chunk.
8. Complete only after the receipt and evidence validate against current inputs.
   After a phase's last chunk, obtain the cumulative Codex phase review
   ([prompt](prompts/review-phase.md)) before starting the next phase.
   Make a focused commit; revalidate that its tree matches the reviewed tree.
   Later edits invalidate affected receipts and trigger targeted regression
   checks/re-review. Preserve failed attempts; never pressure a reviewer to pass.

## Command contract to establish in C01

These commands are a required future interface, not tools that exist today:

```text
cargo run --locked -p xtask -- check --chunk Cxx --manifest docs/architecture/checks.json
cargo run --locked -p xtask -- test --chunk Cxx --manifest docs/architecture/checks.json
cargo run --locked -p xtask -- native --chunk Cxx --manifest docs/architecture/checks.json
cargo run --locked -p xtask -- review --chunk Cxx --reviewer codex --manifest docs/architecture/checks.json
cargo run --locked -p xtask -- review --phase Pxx --reviewer codex --manifest docs/architecture/checks.json
cargo run --locked -p xtask -- complete --chunk Cxx --manifest docs/architecture/checks.json
cargo run --locked -p xtask -- package --kind preview --manifest docs/architecture/checks.json
cargo run --locked -p xtask -- package --kind release-candidate --manifest docs/architecture/checks.json
```

C01 creates the controller and a versioned manifest of exact commands, working
directories, targets, features, fixtures, timeouts, and expected evidence. It
cannot implement the later chunks itself. G01 specifies actual Rust/pnpm/Tauri
commands after choosing the workspace; a future engineer must not guess them or
claim these placeholder commands already ran. Document/test the controller's
rejection of missing, stale, malformed, edited, or sub-threshold receipts.

## Coverage and evidence

- [Feature baseline](feature-baseline.md): existing working/preview/stub behavior.
- [Parity matrix](parity-matrix.md): work ownership and release closure.
- [Dependency gates](dependencies.md): accepted inputs and unresolved qualification.
- [Test catalogue](test-catalogue.md): named behavioral/native/artifact test cases.
- [Review protocol](review-protocol.md): scoring, receipt fields, invalidation.
- [Release contract](release-contract.md): live scope, signing, package/install proof.
- [Mandatory live checks](live-checks.md): fixed L-family/F-smoke requirements and
  owner-approved per-workflow live scope; no access-driven silent reductions.
- [Distribution gate](distribution.md): owner-selected source/binary terms,
  Academy permission, framework disposition and exact shipped notices.
- [Status](status.md): authoritative current state; planned is not implemented.

G01 must convert each chunk's named tests into exact invocation commands and
observable assertions. Tests cannot simply repeat implementation decisions.
Fresh signed-artifact checks bind the final DMG hash and installed bytes; source
or fixture checks do not substitute for them.

## Expected sequencing dependencies

The scan proof can use accepted SDK Phase 3 after native macOS qualification.
Management/DLP uses accepted Phase 4 or a later explicitly reviewed compatible
pin. Existing Red Team/Model reads need verified Phase 5 capabilities; complete
Gateway needs Phase 6 capabilities. Harness/Academy transports are application
work, not delivered by the SCM SDK. Keep these upstream dependencies visible and
do not require unrelated SDK operations merely because they share a library.

First-slice metrics establish named budgets before C04. Every UI chunk includes
keyboard/focus checks. Later introducing chunks register their numeric workload
budgets before implementation/acceptance measurement under T20 governance; budget
changes need recorded rationale and owner or independent reviewer acknowledgement.
C24/C26 compare these pre-registered values, not newly fitted limits. C24 closes
the full native/VoiceOver/performance and cross-feature offline matrix.
Preview packages may demonstrate accepted scope but must name missing features.
Only C26 can establish stable installed-package readiness.

## Completion and delivery

Stable v2 requires every agreed working-v1 parity row closed, all chunk/phase
reviews passing, designated live acceptance complete, no missing mandatory
checks, and C26's installed signed/notarized DMG evidence. V1 stays available;
fresh credentials and reviewed imports remain explicit. Updates initially use
manual signed DMGs with rollback instructions.

The active implementation task may deliver reviewed Git commits to the exact
owner-specified remote using normal pushes. Application release publication,
hosting changes, and promotion over v1 must remain within the owner's explicit
release instruction; preparing a signed package does not by itself publish it.

## P10 — C45: one presentation and fresh identity (D061/D062)

After actual P09all9/audit, C45 implements C45-draft.md and chunks/C45.md. P10=[C45] must pass an actual cumulative independentall9 review before the signed C26/P08release. C26 itself is not required before its own release build; genuine P09/P10 proofs and existing C01–C25 prerequisites guard that production boundary. The approved manual review fallback remains explicit while C26/P08 is open; no controller receipts are fabricated. Verification follows verification-policy.md: one justified C45 baseline, affected followups and reuse for P10 when valid.
