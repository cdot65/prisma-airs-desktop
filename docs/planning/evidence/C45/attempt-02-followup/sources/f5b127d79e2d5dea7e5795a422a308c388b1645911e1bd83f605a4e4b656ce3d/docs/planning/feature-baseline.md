# V1 behavior baseline for v2 scope

Read-only source inventory of Prisma AIRS Desktop 0.11.12, inspected at
`74754c02b19d87187b6dca81f64550a669b0f841`, with retained release evidence.
An implemented source path is not proof of live tenant acceptance or proof that
the future v2 installed application works. SDK dependencies below are planning
families; verify exact operation/option coverage before each chunk.

For every eventual parity row, retain four separate evidence fields: source
implementation, fixture/contract acceptance, recorded live qualification, and
new v2 installed-artifact qualification. Do not turn source inspection into a
runtime test result.

| Feature                   | Existing behavior and qualification limits                                                         | V2 planning classification                                         |
|---------------------------|----------------------------------------------------------------------------------------------------|--------------------------------------------------------------------|
| Tenant setup              | Multiple tenants, protected/session credentials, endpoint overrides, verification/import/reset     | Preserve behavior; new Rust/native implementation                  |
| Runtime scanning          | Prompt/response scan, explicit profile/application, named cases and expected outcomes              | First complete slice; accepted SDK Phase 3 capability              |
| Runtime profiles/topics   | List/detail/create/update/delete, guided fields, advanced JSON, reviewed Apply                     | Working subset; map to accepted SDK Phase 4 contracts              |
| Runtime applications/keys | Application update/delete; key creation/version/rotation/delete; one-time secret reveal            | Working subset; retain uncertainty and secret-handling semantics   |
| DLP                       | Catalog; explicit pattern/profile/filter/dictionary operations; deployment profiles browse         | Preserve exact implemented subset, not universal CRUD              |
| Sessions                  | Session/scan/detector searches, filters, explicit content reveal, protected investigation cache    | Preserve bounds and visible completeness/freshness                 |
| FP/FN reports             | Metadata-only reporting, edit/remove/undo, CSV export                                              | Working reporting; does not change suppression policy              |
| Exception Patterns        | Synthetic literal matching/report/pattern/assignment workflow                                      | Preview; never represent as live policy functionality              |
| Home/Inbox/observability  | Cached facts, key expiry/limits, scoped charts, source freshness/partial/unknown disclosure        | Working; D017 retains operational cost/limit administration        |
| AI Gateway                | Workspace/config/provider/key operations, limits, catalogs, telemetry/logs, streaming Playground   | Working source paths; full SDK family depends on Phase 6           |
| Gateway admin/IAM         | Existing key inventory/reviewed creation and known user identities                                 | Preserve subset; full identity administration remains planned      |
| Red Teaming               | Existing scan inventory/statistics/static reports                                                  | Working read-only observability; new execution remains preview     |
| Model/skill security      | Existing scan inventory/summaries and partial-source status                                        | Working read-only observability; dedicated execution UI is stubbed |
| Test-case Library         | Five-product authoring/specifications, starters, protected save, import/export/catalog import      | Working; only Runtime cases currently execute in Playground        |
| API Client                | HTTP workbench, collections/drafts/history/auth/variables, imports, schemas, Git/profile adoption  | Working and enabled in actual 0.11.12 DMG                          |
| Built-in SCM collection   | 193 raw requests, including IAM/config/TSG/subscription and Prisma product areas                   | Generic workbench coverage; not 193 verified dedicated workflows   |
| Harness                   | Native Rust engine, streaming, model routing, protected history, investigations/proposals/tools    | Preserve native engine; replace host workers and package runtime   |
| Harness sign-in/MCP       | Company SSO/key entry, refresh/recovery, workspace MCP catalogs/sign-in/approval/results           | Working source paths; preserve specific live-evidence limitations  |
| Academy learner           | Guest/account learning, sandbox labs, server grading, Program/exams/certificates, account settings | Share React learner presentation and existing hosted contracts     |
| Academy local work        | Encrypted downloads/drafts with account/access/revision constraints                                | Preserve offline work; grading remains hosted                      |

## Scope details that must survive planning

- Some customized Runtime PI/directionality options are local previews.
- DLP has an implemented operation subset; do not promise operations absent from
  that subset merely because a generic SDK method exists.
- Detector search is bounded to 1,000 sessions / 100 matches. Preserve visible
  completeness or deliberately design and test a replacement, not silent truncation.
- Gateway workspace provisioning spans several operations and can have uncertain
  or partial outcomes. Cancellation or timeout cannot establish rollback remotely.
- Red Team execution actions remain disabled previews; Model Security dedicated
  execution pages remain stubs. SDK expansion does not automatically expand v2's
  approved parity scope.
- Non-Runtime case-library entries are inspectable specifications, not executable
  tests. An import is not proof that its operation can run.
- API Client is off in the default Cargo feature set but explicitly enabled by
  the actual 0.11.12 shipping build. The generic older release script is not the
  evidence for that release's supported feature set.
- Retained MCP notes record a live approved read-only call on a prerelease build;
  the release artifact checks were offline. Live refusal, approval denial, token
  refresh/revocation, and trace identity are separately unverified in that record.
- Academy manager/assessor administration remains web-only. Learner feedback with
  an `admin` procedure namespace is not automatically a manager operation.
- Academy's decorative web motion was not shown/downloaded by v1. Shared React
  presentation may enable it, but offline motion/media support needs an explicit
  capability and bundle assessment.
- D001 removes desktop billing. D017 explicitly retains remote product
  usage/cost/budget/limit views as administrative functionality.

## Principal source references

- [Runtime capabilities](/Users/cdot/development/cdot65/prisma-airs-desktop/src/runtime.rs)
- [Screen availability](/Users/cdot/development/cdot65/prisma-airs-desktop/src/screens.rs)
- [Runtime worker](/Users/cdot/development/cdot65/prisma-airs-desktop/bridge/src/runtime.ts)
- [Gateway worker](/Users/cdot/development/cdot65/prisma-airs-desktop/bridge/src/gateway.ts)
- [Observability worker](/Users/cdot/development/cdot65/prisma-airs-desktop/bridge/src/observability.ts)
- [Case execution rules](/Users/cdot/development/cdot65/prisma-airs-desktop/src/case_library.rs)
- [Actual 0.11.12 shipping build](/Users/cdot/development/cdot65/prisma-airs-desktop/docs/releases/v0.11.12-evidence/shipping_build.py)
- [API Client SCM collection](/Users/cdot/development/cdot65/prisma-airs-desktop/src/api_client/scm/mod.rs)
- [0.11.12 release evidence](/Users/cdot/development/cdot65/prisma-airs-desktop/docs/releases/v0.11.12-evidence/README.md)
- [Academy actions](/Users/cdot/development/cdot65/prisma-airs-desktop/bridge/src/academy.ts)
