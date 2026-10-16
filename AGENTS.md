# Prisma AIRS Desktop v2 agent guidance

## Current stage

This checkout contains planning records, not an application. The owner requested
an explicit interview, a detailed Markdown implementation plan and harness
prompts, then the improve-codebase-architecture workflow, before implementation.
Product interview decisions D001–D023 are accepted. D024 selects Apache-2.0 for
newly authored v2 material; D025 selects all four architecture candidates and
records owner confirmation that their report renders. Read the concrete plan
and current gate state in `docs/planning/status.md`. Continue G01 design/review
under that authorization without repeating blanket product questions.
G01's selected-design review and declared outputs remain prerequisites.
Do not scaffold, port,
install application dependencies, run product endpoints, migrate real data, or
start implementation while those gates are unfinished. Planning documentation
and read-only source research are authorized.

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

Each future implementation chunk requires the agreed checks and a real independent
Claude Code review scoring at least 9/10 overall and in architecture, behavior,
authority/security, and validation, with no blocking/required fixes or missing
mandatory evidence.
Retain source/dependency fingerprints, raw verdicts, commands, test results, and
remediation attempts. Re-review changed source. Never fabricate a score, substitute
self-review, or treat favorable scores as evidence that unrun tests passed.

Follow `docs/planning/IMPLEMENTATION-PLAN.md`, the ordered C01–C26 instructions,
review/test/dependency/release contracts and canonical prompts. G01 specifies
concrete interfaces and exact native/check commands before implementation.
Do not invent these future gates as completed results.
Keep sensitive raw evidence private and repository records redacted.

The owner specified `ssh://git@git-ssh.cdot.io/cdot/prisma-airs-desktop-v2.git` as
this repository's destination. Use normal pushes only when publication is within
the active task; do not force-push, rewrite unrelated repositories, promote v2
over v1, or publish application releases during planning.
