# Prisma AIRS Desktop v2 agent guidance

## Current stage

Planning (G00) and architecture (G01) gates passed. Implementation of the ordered
chunks C01–C26 is authorized and proceeds in numeric order; see
`docs/planning/status.md` for the active chunk and receipts. Product decisions
D001–D023, source terms D024, architecture selection D025 and the Codex review
policy D026 are accepted; do not re-ask settled questions.
A chunk may scaffold, install pinned build dependencies, and add tests only
within its own instructions. Product endpoints are called only under C25's
controlled live scope; real user data is never migrated outside C23's reviewed
import flow.

## Accepted direction

- React presentation hosted by Tauri; focused Rust application implementation.
- Use the Rust SCM SDK directly; no shipped Node/Bun backend runtime or
  TypeScript backend workers. Build tooling and React WebView JavaScript are
  distinct from a shipped backend runtime.
- Academy owns its learner presentation and hosted learning contracts. Share its
  React components/themes/shell where suitable; preserve hosted grading/access
  authority. Academy gamification stays in the Academy feature.
- Selectively evaluate cdot-framework modules and architecture patterns. Do not
  carry media generation, commercial billing, MySQL/Redis, or its server stack
  into the desktop runtime. Preserve upstream provenance/licenses.
- Cover working v1 behavior in testable slices; classify previews/stubs explicitly.
- macOS Apple Silicon first; v2 coexists with v1 using independent application,
  protected-storage, and local-data identities.
- Rust owns credential access, tenant context, request authority, and reviewed
  configuration changes. Harness proposals do not authorize Apply.
- Imports preserve selected user-authored work/settings with fresh credential
  setup and sign-in. Do not copy old approvals/authentication sessions.
- Offline local work is supported; no silent replay of remote operations.
- Remote Gateway usage/cost/budget/limit administration is product functionality,
  not desktop application monetization.

## Source ownership and working safety

- This repository: `/Users/cdot/development/redtail/prisma-airs-desktop-v2`.
- Framework reference: `/Users/cdot/development/redtail/ampersand-services`.
- V1 behavioral reference: `/Users/cdot/development/cdot65/prisma-airs-desktop`.
- Shared Academy: `/Users/cdot/development/redtail/ai-security-academy`.
- SDK: `airs.coder:/home/cdot/development/cdot65/scm-sdk-rs`.
- Desktop-capable native Harness source:
  `/Users/cdot/development/cdot65/prisma-airs-harness-desktop-mcp`.

Read applicable guidance before changing another checkout. Preserve active SDK
work and user changes. Pin accepted external revisions for reproducible inputs;
do not assume evolving worktrees or an older similarly named Harness checkout
match the required contracts. Current research evidence is a snapshot, not live
qualification. Never read or log credentials for planning.

## Reviews and completion

Claude Code implements. Under D026, every implementation chunk and every phase
(cumulatively) requires a real independent Codex review scoring at least 9/10 in
architecture, behavior, authority_security, validation and overall, with empty
required_fixes, blockers and evidence_gaps. Historical G00/G01 Claude reviews
are retained unchanged and cover only their recorded inputs.
Under D034 (superseding D032 for chunks) a chunk completes on a review scoring
at least 8 in all five dimensions with no blockers (`xtask complete
--owner-accept REASON`); the ledger records the acceptance and every accepted
failure (required fixes, evidence gaps). Phase reviews still need 9/10 with no
owner acceptance, and every chunk of the phase accepted below 9 must first be
remediated and re-reviewed to a passing 9/10 (a new generation, later chunks
not reopened); `xtask complete` enforces both.
Retain source/dependency fingerprints, raw verdicts, commands, test results, and
remediation attempts. Re-review changed source. Never fabricate a score, substitute
self-review, edit a verdict, or treat favorable scores as evidence that unrun tests
passed. A queue acknowledgement or process exit status is not a verdict.

Follow `docs/planning/IMPLEMENTATION-PLAN.md`, the ordered C01–C26 instructions,
review/test/dependency/release contracts and canonical prompts. G01 specifies
concrete interfaces and exact native/check commands before implementation.
Do not invent these future gates as completed results.
Keep sensitive raw evidence private and repository records redacted.

The owner specified `ssh://git@git-ssh.cdot.io/cdot/prisma-airs-desktop-v2.git` as
this repository's destination. Use normal pushes only when publication is within
the active task; do not force-push, rewrite unrelated repositories, promote v2
over v1, or publish application releases during planning.
