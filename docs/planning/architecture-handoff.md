# G01 architecture improvement handoff

The product interview and independent G00 plan review are complete; D024/D025
authorize the selected architecture work. This is design/exploration work
before application implementation, not permission to scaffold a prototype first.

## Workflow and source inputs

Use the owner's improve-codebase-architecture skill at
`/Users/cdot/.agents/skills/improve-codebase-architecture/SKILL.md`, its
`HTML-REPORT.md`, the codebase-design vocabulary, and the grilling skill.
Read `GLOSSARY.md`, decisions, this plan, and applicable ADRs first. This new repo
has no application history; scope the scan to the explicitly named source areas
below instead of pretending it has changed-code hotspots.

The skill requires a read-only exploration sub-agent. Read applicable guidance
in each source checkout and preserve active SDK work. Source reference ADRs
explain existing behavior and constraints; they do not automatically become v2
architecture decisions or authorize copying old implementation.

## Exploration priorities

1. Academy-owned shell/controls/themes/learner source versus web account/router/
   manager/gamification coupling. Two real presentation consumers justify a seam.
2. Rust-owned tenant/credential/request/review authority versus presentation and
   SDK transport policy. Inspect v1 transitions and actual SDK contracts.
3. Native host authentication, protected storage, Academy account identity,
   Harness inference sign-in and MCP grants. Seek shared implementation with
   distinct scoped authority, not one generic credential container.
4. Native Harness protocol/sidecar lifecycle, typed host tools, protected history,
   and replacement of TS workers/old package assumptions without engine rewrite.
5. API Client document/transport authority and managed AIRS mutation admission;
   user-controlled generic HTTP cannot bypass the normal Rust review workflow.
6. Explicit outcome/freshness/completeness semantics and import/rollback across
   local work. Put tests at real interfaces, not only extracted pure helpers.

## Candidate report

For each real friction point, identify actual source files/callers, the deletion
test, depth/locality/leverage, a before/after visual, test improvements and
recommendation strength. Clearly distinguish observed source from proposed v2
structure. Do not invent concrete interfaces before the owner selects candidates.
D025 now records selection of all four, so selected-design work proceeds.

Write the skill's fresh timestamped HTML report to the OS temp directory, using
its Tailwind/Mermaid guidance with inline essential styling/static visual fallback.
Core content must render even if CDN scripts fail. Open and visually verify the
actual report; the earlier blank-HTML experience must not recur. Keep report
artifacts outside this repository; record the path/hash and accepted findings.
Ask which candidate(s) to explore, then use the grilling loop to settle each
selected design. Do not select on the owner's behalf solely to bypass that step.

## Required selected-design outputs

- ADRs mapping accepted decisions to chosen deep modules/interfaces, real seams,
  adapters, ownership, error/ordering/performance requirements, and test surfaces.
- Concrete shared package extraction/pinning and regression plan, without a
  divergent Academy copy or leaked learner authority.
- Exact native IPC/command admission and generated contract ownership. Evaluate
  type generation and compatibility with actual pinned dependencies, not an
  assumed package version.
- Independent identity/storage/import schemes and rollback/account rules.
- Native Rust SDK/async-runtime and auth/transport integration, scoped credential
  destinations and unknown-outcome/no-replay semantics. Select a reviewed Rust
  OAuth/OIDC implementation and specify discovery/redirect, claims, algorithm,
  signature/JWKS and key-rotation validation; do not blindly port omitted v1 checks.
- Harness native protocol/resource sealing and no-Node verification strategy.
- Per-chunk exact command/target/feature/evidence manifest and complete per-workflow
  parity expansion. Budget-measurement method and C03 budget gate are explicit.
- Provenance/import decisions; third-party notices and dependency manifests.
- Owner-approved v2 source/binary terms, explicit Academy redistribution scope/
  ownership record and framework-derived disposition under [distribution.md](distribution.md),
  before any code extraction.
- Explicit API Client raw-request admission: classify known Prisma mutation
  endpoints, choose a conservative review/default-denial policy for unclassified
  managed-credential requests, bind destinations/redirects/credentials, and specify
  C15 negative bypass/stale-request tests. Generic IPC admission is insufficient.
- Expanded per-workflow mandatory [live matrix](live-checks.md), with required/
  not-required reasons and separate blocked-access status. Obtain owner approval
  with the selected design before C25; access gaps cannot silently cut scope.
- Hosted OAuth registration dependency map for Academy device clients, Harness
  SSO callbacks and MCP clients: reviewed reuse versus new client/redirect/scope,
  responsible Academy/Gateway/IdP operator and deployment prerequisites. Planning
  access does not authorize production registration changes.

Use the domain-modeling skill if selected design vocabulary requires it; discover
and read its exact skill source then. No new module term should remain undefined
in the glossary. If existing behavior is deliberately changed, record the reason,
product impact and owner decision rather than silently weakening it.

## Qualification timing

G01 fixes architecture interfaces, ownership, first-slice version choices, the
command/test contract and expanded workflow classes. C01 resolves/qualifies actual
locks and shared source; later accepted SDK family pins/options map before their
consuming chunks. This preserves D009 first-slice independence while retaining
every required later gate. G01 does not claim native compilation, live access,
third-party inventory for unconsumed dependencies or final package qualification.

## Exit

Actual independent architecture review must score ≥9 overall and in every
critical dimension with no required fixes. Refine/re-review failed designs.
Update affected chunk instructions and re-review the changed plan. Record the
owner's selected designs and satisfied G01 requirements before C01 can begin.
