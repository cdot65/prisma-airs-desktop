# Behavioral and native test catalogue

These are required future tests and observable assertions, not results already
executed. G01/C01 bind them to exact commands. Each chunk names its applicable
IDs and adds feature-specific cases. Use fake transports/clocks/stores at real
module interfaces plus literal external fixtures; native and live tests provide
separate evidence. Do not assert implementation-private structure for its own sake.

## Core authority and SDK

- **T01 Context isolation:** tenant/workspace/credential switch during a request
  rejects old replies, retires pending reviews/one-time secrets, and cancels work.
  Switching back cannot revive authority. Credential replacement and sign-out
  behave the same way; duplicate completions settle at most once.
- **T02 Review authority:** forged/stale/replayed/consumed review IDs, changed
  draft/resource/context, model text, and imported history cannot Apply. Valid
  operator Apply transmits the exact reviewed operation once.
- **T03 Unknown outcomes:** disconnect before send, after send, timeout, cancel,
  401/403/429/5xx, and partial multistep provisioning yield correct states. No
  automatic mutation/scan replay; offer readback/review/explicit retry where valid.
- **T04 SDK external contract:** independently authored request/response fixtures
  verify method/path/body/headers/options, pagination/multipart limits, unknown
  enum/action preservation, sanitized errors, and no dropped options.
- **T05 Credential confinement:** scan-only setup works; management/inference/MCP
  credentials are separate. Deny wrong-origin redirects or custom endpoint
  changes that would forward a credential without an explicitly reviewed new
  destination binding. Preserve legitimate operator-reviewed endpoint overrides
  and ordinary user-selected current-format collection imports. Alpha/v2 tenant-profile readers and credential-file discovery are removed under D060/C45.
  No implicit environment/tenant file
  loading, upstream fallback, persistent renderer storage, or secret log fields.
- **T06 Protected storage:** locked/denied/unavailable/corrupt store, interrupted
  write/reset, session-only mode, restart, explicit one-time reveal/clipboard,
  and cleanup. No plaintext fallback; v1 namespaces remain untouched.
- **T07 Lifecycle:** bounded concurrent work, cancellation, queue backpressure,
  superseding reads, app close/restart, sleep/wake, expired credentials, and
  monotonic versus wall-clock decisions. UI remains responsive during work.

## Presentation and Academy

- **T08 Shared presentation:** web and desktop consumers build from the same
  reviewed shared source; themes/fonts/routes render correctly. General desktop
  screens mount no Academy gamification/account effects or web manager routes.
- **T09 Native UI:** actual Tauri command path, menus/shortcuts, keyboard-only
  workflow, focus restoration, VoiceOver labels/order, zoom/large text, themes,
  reduced motion, long/empty/error content, and window resizing. Mock IPC tests
  alone cannot establish these. macOS WebDriver availability must be verified;
  use supported native accessibility/automation or recorded manual steps.
- **T10 Academy identity:** device grant, pending/slow_down/expiry/cancel, refresh,
  revocation, wrong issuer/client/scope, account switch/sign-out, and stale replies.
  Token management stays Rust-owned. The learner account is independent of tenant.
- **T11 Learning authority:** client bundles contain no answer keys/private lab
  evaluation, privileged routes, or assessor tokens. Forged verdict/award/local
  exam clock cannot pass; the hosted backend decides access, grading, and awards.
  Signed-in denial differs from guest sign-in; sandbox never uses tenant secrets.
- **T12 Offline work:** download lease/revision/account access, encrypted draft
  save/restart, locked vault, sign-out/account switch, withdrawn content, duplicate
  submission IDs, changed draft versions, and reconnection. No deferred action
  replay. Valid local work remains usable without claiming offline grading.

## Workflows and Harness

- **T13 Collection semantics:** exact capability subsets, large collections,
  bounded pagination/truncation, search/filter/page resets, stale/partial cache,
  consistent Home/Inbox source facts, and no false complete/zero/absent claims.
- **T14 API Client documents:** edit/save/conflict while typing, invalid input,
  environment/secret separation, import/path traversal/size errors, multipart,
  cookies/redirect auth, cancellation, history/download, read-only Git,
  and recovery from interrupted writes. Arbitrary workbench targets cannot borrow
  privileged application credentials through renderer-supplied references.
- **T15 Harness process:** actual native binary and bounded bootstrap/JSONL
  protocol, malformed/oversized frames, correlation/duplicates, queue pressure,
  crash/EOF/cancel, context change, graceful/forced cleanup, and missing/tampered
  resources. No PATH launcher fallback or plaintext rollout/history.
- **T16 Harness authority:** typed investigation/proposal tools, active-draft
  protection, no engine Apply, one-time secrets absent from model context,
  read-only project defaults, approved single-process command restrictions,
  forbidden descendants/pipelines, stale decisions, and history replay.
- **T17 Inference/MCP identity:** pinned Gateway origin; SSO PKCE/state/nonce/
  issuer/client/audience/time/signature/JWKS/algorithm/key-rotation validation;
  sleep/refresh/revoke/identity changes;
  distinct inference/MCP grants; fixed conversation server scope; exact call-item
  approvals; changed revision/grant; denied tools; trace/execution evidence.
  No direct upstream provider fallback or global cross-workspace MCP authority.

## Import, quality, and artifacts

- **T18 Fresh setup and ordinary file isolation (C45/D060):** bootstrap in the new application/data/credential identities without reading, adopting, replaying or purging alpha/v2 stores. Synthetic old-root/service canaries survive new setup, restart and reset; old stores/vaults receive no reads. Ordinary user-selected API collection/cURL/Insomnia/case files retain their formats, bounded parsing, traversal refusal, reviewed selection and source-byte preservation. The former alpha transaction, tenant-profile import and API profile adoption are intentionally retired; historical results remain unchanged.
- **T19 Diagnostics privacy:** seeded canary secrets and customer content absent
  from logs/crash files/exports by default; explicit redacted export; correct
  retention/reset; no analytics/crash upload attempts; encrypted user history
  remains a distinct intentional storage feature.
- **T20 Performance:** named macOS hardware/OS/workloads, cold/warm startup, idle
  and scan/large-list/streaming/Harness memory, main-thread responsiveness, file
  and DMG footprint. Record methodology, repetitions, distribution and baseline;
  set core budgets in C03 and register later workload budgets at the introducing
  chunk's entry before implementation/acceptance measurement. See the budget
  governance below; C24/C26 compare with pre-registered values.
- **T21 Controlled live:** designated account/tenant/entitlements and disposable
  resources, explicit operation allowlist/budgets, sanitized request IDs, readback,
  cleanup receipts, deny/error cases, and unresolved outcome handling. Missing
  access or unsupported operations remain blocked; no destructive production tests.
- **T22 Installed package:** signed/notarized/stapled DMG, Gatekeeper/quarantine,
  mount/drag/install/launch/relaunch with fresh user/home and no developer checkout,
  PATH Node/Bun/managed CLI disabled, actual Rust sidecar, runtime inventory/hash
  integrity, no hidden backend download, safe uninstall/reset/coexistence,
  manual update, previous-version rollback, and preserved user work.
- **T23 Cross-feature offline:** launch offline and read/edit each applicable
  local category: saved cases, FP/FN reports, API Client documents, protected
  Harness history, configuration drafts, and Academy work. Mark remote data as
  stale/unknown as appropriate. Offline drafts do not retain valid Apply authority;
  reconnect requires fresh readback/review when needed. Pending reviews, scans,
  inference turns, generic HTTP mutations and MCP/tool calls never resubmit on
  reconnect. Offer explicit retry while preserving uncertain remote outcomes.
  Bind to C03,C09–C12,C14–C15,C17–C18,C20–C22 and cumulative C24.

## Performance budget governance

C03 registers numeric budgets and named methods for the shell/scan workloads
before C04. Also register the later workload catalogue and owning chunks.
Before implementing/measuring a newly introduced workload, its owning chunk
commits a numeric budget, hardware/OS, dataset, measurement method and rationale:
C11/C13 large collections; C15 large HTTP bodies/downloads; C18 streaming/charts;
C19 native-engine startup/memory; C20 history; C21/C22 tool/MCP workloads.
Do not wait until C24 to fit limits to observed results.

A budget change requires a versioned rationale, original failures/results, and
owner or independent reviewer acknowledgement recorded in the receipt. It cannot
silently erase a regression. Benchmark evidence records the pre-registered budget
hash; the controller rejects retroactive or mismatched budget inputs. C24/C26
compare against those exact pre-committed values, including changed-budget history.

Normal bounded metadata reads or credential refresh prerequisites for an explicit
operation are not deferred business-action replay. Tests must distinguish these
from silently resubmitting scans, writes, grading, or tool calls after reconnect.

Before review, challenge security-critical tests with a relevant deliberate
behavior mutation or equivalent negative probe. Show that violating an invariant
fails the test; do not create an exhaustive mutation framework or mirror every
implementation detail. Run mutations only in isolated disposable copies.

C45/P10 checks follow verification-policy.md: record affected behavior/dependencies, selected checks, retained original artifact bindings, uncertainty and the reason for any full baseline. One complete C45 identity/isolation/cleanup baseline is justified; follow-ups/merge/P10 reuse valid coverage with explicit proof or unchanged-impact rationale and execute affected targets. Every required assertion is accounted; no omission or score substitutes for evidence.
