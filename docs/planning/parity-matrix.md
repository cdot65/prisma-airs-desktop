# Working-v1 parity and release closure

All v2 rows are **planned**. No v2 source/native/live/artifact result exists yet.
The baseline is [feature-baseline.md](feature-baseline.md); do not expand a
preview/stub into required live behavior without an explicit scope decision.

| Requirement family                     | Owning chunks   | Closure requirement                                                                  |
|----------------------------------------|-----------------|--------------------------------------------------------------------------------------|
| Tenant setup/scan-only/protected state | C02,C03,C08,C23 | Context/capabilities, reviewed endpoint overrides/profile import, verification/reset |
| Runtime synchronous scan/cases         | C03,C12         | Faithful verdicts, profiles, cancellation and no silent replay                       |
| Profiles/topics/applications/keys      | C08,C09         | Exact supported reads/writes, review binding, one-time secrets                       |
| DLP/deployment catalogue               | C08,C10         | Exact supported operation subset and real multipart semantics                        |
| Sessions/content/search/cache          | C11             | Explicit reveal, protected persistence, bounded completeness                         |
| Metadata FP/FN reports                 | C11             | Edit/remove/undo/export without suppression side effects                             |
| Home/Inbox/source observations/charts  | C12,C18         | Runtime and Gateway data, freshness/partial/unknown/attention                        |
| Five-product Library/specifications    | C12             | Authoring/import/export; only supported Runtime execution                            |
| Red Team read-only observations        | C13             | Accepted actual reads; no invented live execution                                    |
| Model/skill read-only observations     | C13             | Accepted actual reads and partial-source disclosure                                  |
| API Client documents/projects/imports  | C14             | Fresh-input reconciliation, drafts/conflicts/secrets/Git adoption                    |
| API Client transport/history/catalogue | C15             | Real HTTP/cookies/downloads, scoped auth, no mutation bypass                         |
| Gateway workspace/config/admin subset  | C16,C17         | Supported scope/options and reviewed multi-step outcomes                             |
| Gateway inference/logs/telemetry/cost  | C18             | Streaming, route/model selection, operational budgets/limits                         |
| Harness native run/stream/lifecycle    | C19             | Real Rust engine/protocol, cleanup and no packaged Node runtime                      |
| Harness identity/protected history     | C20             | Rust SSO/key lifecycle, scope-safe restore/export/delete                             |
| Harness typed tools/project permission | C21             | Investigation/proposals and enforced existing sandbox restrictions                   |
| Workspace MCP                          | C22             | Rust auth/grants, exact call approvals and execution evidence                        |
| Academy identity/account isolation     | C04             | Device OAuth, scope admission, tenant-independent learner identity                   |
| Academy learner presentation           | C05             | Shared catalog/lessons/checks/sandbox/UI; feature-local gamification                 |
| Academy Program/exams/certificates     | C06             | Hosted access/timing/grading/award authority and account workflows                   |
| Academy encrypted offline work         | C07             | Lease/revision/account-aware reading/drafts; explicit submission                     |
| Reviewed imports/fresh authentication  | C23             | User work, source preservation, restart/rollback; no old approvals                   |
| Native UX/privacy/measured performance | C01–C24         | Per-chunk checks plus cumulative actual native quality closure                       |
| Controlled live behavior               | C25             | Designated scope, real outcomes, readback/cleanup, honest gaps                       |
| Signed installed package/update        | C26             | DMG/notary/Gatekeeper, exact bytes, fresh user, coexistence/rollback                 |

## Required per-workflow ledger

Expand each family into individual existing workflow/operation rows in G01/C01.
Every row needs an ID, baseline source, current classification, responsible chunk,
exact SDK/shared dependency contract, and independent expectations. Preserve four
evidence categories separately: source implementation, fixture/contract results,
native/live qualification, and final installed-artifact qualification. Record
denied/unsupported/partial results honestly rather than collapsing them to pass.

G01/C01 adds a live_requirement (required/not_required with reason) and separate
ready/blocked-by-access status to every row under [live-checks.md](live-checks.md).
The owner approves the expanded list before C25. Missing access cannot shrink
mandatory scope; a reduction requires a recorded owner decision and new review.
T23 cross-feature offline acceptance applies to local work and pending operations,
not just Academy. C24 closes that cumulative matrix.

V2 source completeness alone does not close a row. Each workflow needs applicable
success and failure assertions, context/authority coverage, native acceptance and
required live/artifact evidence. C25/C26 review the cumulative ledger against
the final frozen input set. A required blocked check blocks stable completion.

## Intentional non-goals and retained classifications

- Financial desktop billing/monetization and media-generation infrastructure are
  excluded. Remote operational cost/limit administration is retained.
- Exception Patterns remains explicitly synthetic/inert if shown. FP/FN reports
  do not suppress scans or modify policy.
- New Red Team execution and dedicated Model Security execution remain outside
  parity. Their existence in the new SDK does not authorize adding product scope.
- Academy admin/assessor operations remain web-only. Learner feedback with an
  administrative namespace is classified by actual authority, not string matching.
- No browser app, in-app updater, or additional supported OS/architecture is
  required for first stable v2.
- Shared Academy appearance replaces the GPUI presentation implementation. Do
  not infer a pixel clone or identical internal widget/theme catalogue. Preserve
  theme preference workflows and specify reviewed mappings for imported settings.
- The generic SCM workbench catalogue does not prove all 193 requests were live
  tested. Classify its supported execution/admission separately from dedicated
  workflows and bind its request metadata to a reviewed source snapshot.
