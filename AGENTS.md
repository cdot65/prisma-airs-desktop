# Prisma AIRS Desktop agent guidance

## Current work

The owner confirmed the 2026-11-04 refinement plan: see
[the accepted scope](docs/planning/refinements/2026-11-04/plan.md),
[verification plan](docs/planning/refinements/2026-11-04/verification.md) and
[execution status](docs/planning/refinements/2026-11-04/execution.md).
P11 comprises C46 connection/diagnostics, C47 shell, C48 API Client and C49
Settings/themes. Use focused checks and the affected native flows. Historical
P09/P10 outcomes, sealed evidence and the published v1.0.0-rc.1 stay unchanged.
The RC remains a prerelease; full installed live acceptance is deferred by the
owner. No new live API calls are authorized by this refinement task.

## Execution location — owner correction, 2026-11-04

Interactive automated development, local builds, focused checks and native UI tests run inside the
dedicated Worf macOS VM, reachable as `admin@worf-vm` (formerly `airs-vm`). The
current isolated checkout is
`/Users/cdot/development/redtail/p09-queue/refinement-20261104/core`, on
`refinement/20261104-vm`. Preserve the other VM checkouts and their user changes.
The separately approved CI/release builder roles remain unchanged: Enterprise
for Linux AMD64 and Jadzia initially for macOS ARM64, as in the build/release plan.

Kirk is for the owner's live acceptance. Do not drive its desktop or start native
development/test launchers there unless the owner explicitly changes this rule.
The former Kirk P11 launcher and supervisor are retired. SSH/SCM transport through
Kirk, when needed for the VM's private Git service route, performs no application
builds, tests or UI actions.

Use the VM's pinned tools and its granted `agent-desktop` for owned-window
activation. Bind every native action to the exact fixture bundle, PID and process
lifetime. Existing source checks executed on Kirk before this correction retain
their actual original provenance; do not relabel them VM runs or replay unrelated
suites merely to refresh their source fingerprint.

## Historical baseline

P09 attempt-02 passed actual independent9/9/9/9/9 and its audit on2026-11-02.
C31attempt07 andC32attempt23 passed normally at all9; C30/C41 retain explicit owner
standing with raw historical scores unchanged. C45/P10 is the approved current
implementation: one presentation, Prisma AIRS Desktop1.0.0, new io.cdot.airs-desktop
identities and fresh setup without alpha/v2 data or credential reads/migration.
C26/P08 remains open. Later owner authorization produced the signed/notarized
v1.0.0-rc.1 prerelease; do not interpret that as stable acceptance.

## Verification policy for C45/P10 and future work

Use [verification-policy.md](docs/planning/verification-policy.md). Plan checks
from changed behavior/dependencies/boundary risk. C45 warrants ONE broad current
identity/isolation/cleanup baseline after focused preflight/cross-consumer review.
Follow-up fixes/merges and P10 use affected checks and explicit exact-artifact or
unchanged-impact evidence carry; a commit, worktree, review, score or source hash
alone never triggers another full native/Rust suite. Original raw results and
artifact bindings remain unchanged. Every required assertion/check is accounted.
Historical P09 verifiers/results remain untouched. Actual independent chunk/phase
review and release requirements remain in force; this policy grants no live calls.

Product endpoints are called only within explicit controlled live scope. L19’s
original two plus separately approved third reads are all charged; no additional
L19 live call is authorized. The next fixture gates need no real credentials.
Do not copy host Keychains, private manifests, CLI tenant TOMLs or auth profiles.

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
- D060 removes alpha import from the one presentation. C45/P10 removes the
  remaining alpha readers/contracts and starts the new namespace with fresh
  setup; no alpha/v2 data, credentials or approvals are migrated.
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

Most original implementation was Claude Code; the owner authorized Codex for
the current integration and remediation. Under D026, every implementation chunk and every phase
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
