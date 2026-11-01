# Mandatory live and final-artifact check list

This fixed requirement list is part of G00's concrete plan confirmation. G01
expands it into individual parity rows with exact operations/assertions; the owner
approves that expanded list with the selected architecture before C25. Neither
access limitations nor a favorable review may silently shrink it.

Each expanded row carries `live_requirement = required | not_required` and a
reason for `not_required`. Access status is separate: ready or blocked-by-access.
Blocked access cannot turn required into optional. Any reduction needs a recorded
owner scope decision and revised plan/review. This list authorizes no current
endpoint calls; execution uses C25's designated account/tenant/operation/budget/
cleanup manifest under the active task.

## Required C25 real-service families

- **L01 Runtime scan:** scan-key success with explicit profile/application,
  actual verdict, denied credential and no management-registration prerequisite.
  Record tested region/endpoint scope. Unknown enum behavior is an independent
  fixture assertion, not an invented live server response.
- **L02 Management identity/reads:** designated management OAuth, least-privilege
  denial, supported Runtime/application/key/DLP inventories/details, pagination
  and explicitly approved endpoints. No cross-capability credential use.
- **L03 Runtime changes:** each supported profile/topic/application/key operation
  class, including version/rotation/delete where advertised, on disposable
  resources with review, one-time-secret handling, readback and cleanup.
- **L04 DLP:** exact supported pattern/profile/filter/dictionary operation classes,
  real multipart upload/replace/delete where supported and deployment catalog
  read. No universal CRUD is inferred.
- **L05 Sessions:** known synthetic scan/session records, filters/search/details,
  explicit content authorization and correct completeness. Never use unrelated
  customer session content as test fixtures.
- **L06 Red Team/Model/skill reads:** existing authorized seeded test records for
  advertised inventory/statistics/report reads. No new adversarial/model scans
  are required to qualify read-only parity; missing records/access block required
  evidence rather than creating a fake pass.
- **L07 Gateway reads:** workspace/config/provider/key/limit/catalog/routing/model
  reads, least-privilege denial and selected-workspace prerequisites.
- **L08 Gateway changes:** each supported reversible configuration/provisioning/
  key/limit operation class, including multi-step IAM/workspace settlement,
  readback/reconciliation and cleanup of explicitly disposable resources.
- **L09 Gateway inference:** bounded streaming Playground/inference through the
  intended Gateway and selected routing/credential context, cancellation/error
  recovery and no direct upstream fallback. Record consumption limits/budget.
- **L10 Operational observation:** Runtime/Gateway Home/Inbox/log/telemetry/usage/
  cost/budget/limit sources against designated data, truthful empty/partial/
  freshness/denied states and bounded eventual-consistency polling.
- **L11 Academy identity/access:** device sign-in, learner account, protected
  procedure admission, guest versus signed-in denial, refresh/revoke and account
  switch using authorized test identities; no manager authority in desktop.
- **L12 Academy learning:** real client-safe lesson/catalog reads, knowledge
  check/lab/progress submission and hosted grading on synthetic test coursework.
  A locally forged verdict cannot pass; no tenant secrets enter sandbox.
- **L13 Academy assessment/account:** test-account exam/practical/certificate
  and preferences/avatar/export operations for the working subset, with server
  timing/access/award authority. Use staging/synthetic certification data, not
  irreversible real learner credentials or awards.
- **L14 Academy local-work contracts:** real client-safe download/lease, explicit
  draft submission receipt, account lock/revalidation and denial after an
  authorized fixture access change. Lease expiry extremes also use deterministic
  contracts; a fake server is not live evidence.
- **L15 API Client:** actual designated HTTPS endpoint for headers/cookies/
  uploads/downloads plus representative approved AIRS read/reversible-write
  requests. Mutation review/credential binding cannot be bypassed. The 193
  catalogue entries are specifications, not all claimed live executions.
- **L16 Harness:** actual native engine over designated Gateway inference,
  key and company-SSO modes, bounded conversation/investigation, scope-safe
  restart/history and operator proposal review. Protected disk/process and
  local sandbox assertions also require native evidence, not network alone.
- **L17 Workspace MCP:** real designated Gateway/MCP sign-in/discovery, approved
  read-only call, refusal/approval denial, refresh/revocation, changed scope/grant
  and trace/call-item correlation. V1 discovery/read evidence does not close its
  formerly unverified denial/refresh/revocation cases.
- **L18 Scope isolation:** distinct designated tenant/workspace/credential/account
  scopes verify intended destination and denied cross-scope use. Native race
  tests complement live authority evidence. Do not use production or collapse
  identical scopes and label them cross-tenant qualification.

Each supported operation/options row maps to L-ID(s) or the explicit
not-required categories below. Map exact supported operation classes in G01/C01;
the prose is a minimum, not permission to omit difficult supported writes.

## Optional C29 wizard family L19

L19 is a separately designated, read-only wizard qualification on Vulture
(test-b, TSG 1852583913, US). It is not part of C25's default L01–L18 execution
set. Existing manifests with those eighteen families remain valid; an absent
L19 is reported as blocked, never inferred from L18's scan permission.

The management credential verification currently performs
`profiles().list(limit=1, latest=true)` and discards the returned profile contents.
It is not a token-only check. The explicit L19 operation is:

> one management credential verification through the first-run wizard (C29): one latest security-profile probe with limit=1; discard profile contents; no other inventory browsing or writes

L19 requires one `client_secret` Keychain reference, nonsecret `client_id`,
`tsg_id` and `region`, and a `reads` budget of one or two with no other budget.
No disposable resources or remote cleanup are permitted. The operator resolves
only the exact reference in the approved private manifest and selects only
Runtime Security in the wizard; its scan-key check is local storage validation,
not a scan. Gateway, Red Team and Model selection would admit additional reads
and is outside this designation. Do not complete onboarding into a view that
automatically refreshes inventories.

Before execution, validate the private manifest with `xtask live-scope`. Retain
the source/build/manifest hashes, wizard verification status, nonsecret client
and TSG identifiers, bounded operation metadata and local credential cleanup
result. Retain no secret, token or profile response. A scope report or synthetic
verification alone does not close C29's live row. See its
`evidence/C29/live-scope-request.md` for the current authorization state.

## Explicit not-required live categories

- **N01 Local-only work:** menus/themes/accessibility, local case/report/document
  edit/save, protected history inspection, import/rollback, sandbox policy,
  diagnostics and performance need native/fixture/artifact evidence, not unrelated
  hosted calls. Their remote execution paths still map to L01–L18.
- **N02 Preview/stub behavior:** inert Exception Patterns, new Red Team execution,
  dedicated Model execution, unsupported DLP and web-only Academy administration
  are not live parity claims. UI must disclose/deny them.
- **N03 Input/state permutations:** malformed/oversized frames, future unknown
  enums, corrupt files, lost-response timing and draft races use deterministic
  independent fixtures/native probes. Representative real-service success/denial/
  outcome remains required for the owning L-family.
- **N04 Raw request specifications:** catalogue metadata/import parity needs source
  and literal contract checks. Executing every catalogue request exceeds the
  generic workbench claim and permitted live scope; L15 proves designated actual
  execution/admission.

## Fixed C26 final signed-artifact smoke set

Run against exact signed/stapled DMG and installed inventory hashes, with bounded
designated accounts/resources, no developer checkout and no Node:

1. **F01** Quarantined install/launch/relaunch; scan-key setup and real Runtime scan,
   explicit verdict/error/cancel display, tenant isolation and safe logs.
2. **F02** Academy device sign-in, hosted client-safe lesson, valid offline
   draft/read/restart and explicit synthetic-coursework submission.
3. **F03** Management read and reviewed disposable configuration change,
   readback/cleanup, context review invalidation and no raw HTTP bypass.
4. **F04** Gateway routing/streaming and operational usage/observation view using
   intended inference credential/budget without upstream fallback.
5. **F05** Actual native Harness conversation, Rust company-SSO/session path,
   typed investigation/proposal, protected history, MCP approve-once/read and
   denied-call evidence, process cleanup and no hidden runtime download.
6. **F06** API Client actual HTTPS request/result/download, save/restart/history,
   scoped credentials and selected workbench mutation review.
7. **F07** Library/report/local-work persistence and cross-feature offline/reconnect:
   no queued scans/configuration/inference/tool/grading replay.
8. **F08** Reviewed synthetic v1 import, locked/denied store, fresh sign-in,
   coexistence/reset/uninstall source preservation and rollback.
9. **F09** Menus/keyboard/focus/VoiceOver/themes, budget smoke, signature/notary/
   staple/Gatekeeper/resource-tamper checks, manual forward/backward installation
   and preserved work with declared schema compatibility.

C25 supplies comprehensive L-family evidence. F01–F09 prove critical real paths
and install/native authority on final bytes; they do not replace the full earlier
matrix. Any artifact/source/build-flag change requires affected revalidation and
a newly bound final review.
