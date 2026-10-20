# C17 remediation after attempt-01

Verdict attempt-01 scored architecture 9, behavior 7, authority/security 8,
validation 7 and overall 7 with six required fixes, one evidence gap and no
blockers. D043 and the DESIGN C17 section record the decisions below.

## R1: a workspace switch retires waiting applies

Each apply now shares its progress (`Progress`: waiting, retired, sent,
written) with its record, which also keeps the `WorkspaceRef` it was
reviewed under. Every selection change (`choose`, losing the selection,
a tenant context change) retires the reads of other references and marks
every unsettled change bound to another reference that is still waiting as
retired. The job's dispatch checks and marks its progress under the same
lock, so a retired change never writes: it settles not sent with the new
failure `workspaceChanged`. Choosing the first workspace again issues a new
reference; the old one stays retired. A write already dispatched keeps its
own outcome. Tenant and credential retirement are unchanged (the operation
is cancelled; a waiting job settles not sent). Test:
`choosing_another_workspace_before_the_write_sends_nothing` (the apply's
baseline re-read held, then A→B→A: not sent, nothing written to A or B; a
dispatched write held, then A→B: completed in A, written once).

## R2: the all-models setting is read back

`models_show` compares every requested model's enablement and, when the
draft sets `allow_all_models`, the service's value. Tests: the unit
`a_models_readback_needs_every_requested_model_and_the_all_models_setting`
(literal model replies for both values, an ignored setting with matching
rows, a row not taken) and the integration
`a_models_readback_checks_the_all_models_setting` (both values taken: found;
the service keeps all models allowed: differs).

## R3: provisioning summaries claim only what was read back

`Provisioning` now carries the SDK's scope cleanup (`notNeeded`, `retained`,
`deleted`, `failed`, `unknown`; a reused scope is never this attempt's) and
`verified()`. The presentation's `provisioningText` calls a provisioning
verified only when every step completed and the workspace, its IAM scope
and the binding all read back; otherwise it names what reading the tenant
back did not confirm. Uncertain, partial and refused results state the
cleanup instead of "nothing was undone". The review's impact text states
the SDK's cleanup policy. Tests: `a_failed_cleanup_is_reported_and_never_called_undone`
(refused delete: failed, the scope remains; lost delete: unknown) and the
existing provisioning tests now assert the cleanup; App test
`calls a provisioning verified only when reading the tenant back established it`
(ten cases: failed, incomplete or ambiguous reconciliation, an unbound
scope, deleted, refused and unknown cleanup).

## R4: reconciliation finds this attempt's workspace

Reconciliation keeps the identity the SDK returned and looks for that
workspace. Without one, it takes the single workspace with the requested
name and scope that did not already use a reused scope. Those workspaces
are read just before the write, and if that read fails, no workspace is
attributed. More than one candidate, or no prior list, is reported as
`ambiguous` and nothing is attributed. The binding is checked only for the
identified workspace. Test:
`a_reused_scope_never_attributes_another_workspace_to_the_attempt` (Team B
listed first on the reused scope: a lost create finds Team E unbound; a
refused binding reports Team F unbound; an unreadable prior list with a lost
create is ambiguous).

## R5: an answered write survives a stopped readback

Once the write is answered, its output (outcome, reply, one-time key) is
stored in the shared progress before the readback starts. A cancellation
then returns it with readback `stopped`, and the key stays held for its
single native reveal. This applies to Runtime and DLP changes too. Before
dispatch a cancellation is still not sent, and an unanswered write is still
unknown. Tests: `stopping_during_the_readback_keeps_the_answered_write_and_its_key`
(Gateway service key) and
`stopping_during_the_readback_keeps_the_completed_write_and_its_key`
(Runtime API key, the shared path); the existing cancel tests still pass.

## R6: drafts and uncertain results are kept on this Mac (T23)

New `gateway_local` and `authority/gateway_local_domain` modules add one
owner-only file per tenant under the sessions root, removed with the tenant
and by a reset. Intents `OpenGatewayLocal`, `SaveGatewayDraft` and
`DiscardGatewayLocal` manage it. The workbench keeps edited text once typing
pauses. The core labels each new draft with the workspace a review would
apply to and refuses credential fields. A settled change that sent
something drops its draft. One that settled unknown, partially or without a
confirming readback is recorded with the reviewed text, its outcome,
readback and steps. Nothing kept carries a review, confirmation, reply, key
or authority. The AI Gateway page lists the kept work even when the service
cannot be reached. A draft opens for editing, but it is reviewed only under
its own workspace chosen from a fresh listing. Reconnecting sends nothing,
and a record goes only when dismissed.

Tests:
- `drafts_and_uncertain_outcomes_survive_an_offline_restart_and_nothing_is_replayed`:
  Core restarted offline and then online. The draft and the provisioning
  record are restored. The draft is edited offline. The old review is
  refused and nothing is sent. Reconnecting writes nothing, and an explicit
  fresh review writes once.
- The `gateway_local` unit tests.
- The App test `keeps drafts and uncertain results on this Mac, readable offline and reviewed again only when asked`.
- Contract validation.
- Native checks C17-N11 (offline relaunch) and C17-N12 (reconnect).

## G1: every advertised operation through the production interface

`every_advertised_operation_sends_its_literal_request_once_and_settles_by_outcome`
runs each D043 operation that no other test covers through review, the
native confirmation and apply against the stateful synthetic Gateway. The
operations are: workspace update, provider create, route create and delete,
guardrail update and delete, service-key update and delete, user-key
create, update, rotate and delete, and integration create, update and
delete. Each one is checked three ways:

- **Refusal:** rejected, sent once, nothing read back.
- **Fresh review:** exactly one write with its literal method, path (an
  integration delete carries `organisation_id`), tenant header and body;
  the readback; the workspace shown; the one-time key for creates and
  rotations.
- **Lost reply (fresh desk):** unknown, no key, never retried.

## Probes

Thirteen new mutation probes guard R1–R6. The probes whose code moved were
updated: the models readback, the reconciliation call, the not-sent stop
and the dispatch mark.
