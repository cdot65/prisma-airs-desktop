# Prisma AIRS Desktop v2 planning

Status: product interview complete through D023; concrete plan prepared for G00
review and final shared-understanding confirmation. G01 architecture work remains
unstarted. Application implementation is not yet authorized to begin.

The intended product is a fresh Prisma AIRS desktop application with React
presentation and a Rust application implementation using `scm-sdk-rs`, hosted
by Tauri. The existing GPUI application supplies
behavioral reference and acceptance requirements; it is not the implementation
to translate wholesale.

## Agreed direction

- Public, noncommercial distribution. No sales or monetization requirement.
- No billing constructs in the v2 application. Academy's gamification belongs
  only to the Academy feature, not to tenant/product administration or Harness.
- Eventual coverage of every working v1 workflow, including Harness and Academy.
  Inventory working, preview, unfinished, and retired behavior separately.
- First supported platform: macOS Apple Silicon, with a signed and notarized DMG.
  Other platforms require explicit later milestones.
- Reuse Academy's remote learning interactions and portable React learner
  presentation. Academy remains the source of truth for that presentation and
  hosted learning behavior.
- Academy's React controls, themes, and shell layout are the preferred starting
  point for the broader desktop UI, subject to architecture verification. Shared
  styling does not make the broader application an Academy product or extend
  its learning economy to other features.
- Desktop delivery is the scope. No browser application is required for v2.
- Use the replacement Rust SDK instead of the TypeScript SCM SDK and its bridge.
- Adapt cdot-framework selectively; omit its media/billing/server infrastructure
  from the desktop runtime. Evaluate exact code imports for suitability/provenance.
- First complete slice: shared desktop shell, tenant credentials, Rust SDK Runtime
  scan, and verdict presentation, including cancellation and stale-context tests.
- Ship no Node/Bun runtime or TypeScript backend workers. React remains JavaScript
  in the WebView; preserve the native Rust Harness engine where suitable.
- Install v2 alongside v1 with independent identity, protected credentials, and
  local data. Support explicit reviewed imports and rollback; share the existing
  remote Academy learner account and progress.
- Dedicated repository: `ssh://git@git-ssh.cdot.io/cdot/prisma-airs-desktop-v2.git`;
  checkout `/Users/cdot/development/redtail/prisma-airs-desktop-v2`.
- Scan-key-first onboarding; management OAuth is a separate credential capability.
- Rust owns application authority; context-bound configuration reviews and the
  existing Harness project/tool restrictions survive the presentation change.
- Import selected user-authored work/settings with fresh authentication. No old
  approvals or authentication sessions transfer; history compatibility is explicit.
- Offline local work is available, but remote actions do not silently replay.
- Preserve Gateway usage/cost/budget/limit administration while removing desktop
  financial billing and monetization.
- Complete the interview and Markdown implementation instructions/prompts, then
  run the architecture improvement review, then implement, review, test, validate,
  and sign the selected design.
- Require independent Claude Code scores of at least 9/10 overall and in every
  critical dimension. Passing reviews do
  not replace executable acceptance tests, artifact verification, or resolution
  of blocking findings.

## Planning records

- [Implementation plan](IMPLEMENTATION-PLAN.md): ordered phases and execution contract.
- [Chunk instructions](chunks/README.md): 26 individually scoped implementation chunks.
- [Dependency gates](dependencies.md), [parity matrix](parity-matrix.md),
  [test catalogue](test-catalogue.md), and [review protocol](review-protocol.md).
- [Release contract](release-contract.md) and [architecture handoff](architecture-handoff.md).
- [Mandatory live/final-artifact checks](live-checks.md) and [distribution gate](distribution.md).
- Harness prompts: [implement](prompts/implement-chunk.md), [review](prompts/review-chunk.md),
  [remediate](prompts/remediate.md), [plan review](prompts/review-plan.md),
  [architecture](prompts/architecture.md), [installed release review](prompts/release.md).
- [Status](status.md): current gate/review state; planned work is not implemented.
- [Decisions](decisions.md) separates accepted decisions from proposals.
- [Source evidence](source-evidence.md) records inspected revisions and constraints.
- [Feature baseline](feature-baseline.md) classifies working, preview, and stubbed
  v1 behavior and separates source evidence from runtime qualification.
- [Decision tree](decision-tree.md) tracks the current frontier and interview exit.
- [Domain glossary](../../GLOSSARY.md) fixes the meaning of shared product terms.

## Remaining gates

G00 needs final confirmation of shared understanding of this concrete plan.
G01 runs the requested visual architecture exploration, owner candidate selection
and design grilling, ADR/interface/command specification and independent review.
Only then can C01 begin. SDK/native/live/import/signing dependencies are explicit
future gates; none is represented as already verified by the planning documents.
Do not mark a chunk complete because a reviewer supplied a favorable number.

Keep the completed educational rebrand and its evidence intact. These records
start a separate planning effort; they do not reopen or alter its phase receipts.
