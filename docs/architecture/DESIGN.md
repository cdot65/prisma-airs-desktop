# Prisma AIRS Desktop v2 selected architecture draft

All four candidates were selected by the owner on 2026-10-16. These are concrete
implementation recommendations for that selection, not application source or a
claim that G01 has passed. Product constraints D001–D023 remain unchanged.

## Module ownership

| Module | Owns | Does not own |
|---|---|---|
| Rust application core | Tenant/context admission, request coordination, configuration reviews, remote outcomes, protected work | Hosted grading or raw editor keystrokes |
| Tauri native host (desktop-host) | Window/origin/schema IPC admission, permission manifest/CSP, native Keychain/AppKit sheet/pasteboard adapters and process bootstrap | Domain review policy or hosted learning truth |
| React presentation | Immediate input/undo, layout, accessible controls, navigation and redacted state | Stored credentials, execution permissions, request settlement |
| Shared Academy presentation | Academy-owned controls, themes and shell/learner presentation reused by web and desktop | Desktop tenant authority or global learning economy |
| Academy feature | Learner composition, account-scoped client-safe content/work and hosted learning interactions | AIRS tenant credentials, manager routes, local grading truth |
| Native Harness lifecycle | Sealed engine, scoped bootstrap/grants, process supervision, restricted tools/history | Operator Apply or new authentication authority |
| Workbench document | Fresh input, revision/save/conflict/protection semantics | Permission to borrow tenant credentials |
| Workbench execution admission | Fresh request classification, credential/destination/review binding | A generic escape from native authority |
| SCM SDK adapter | Exact accepted SDK transport contracts/options and outcome translation | Interactive sign-in, OS storage or UI workflow policy |

Start with desktop-core, the desktop-host Tauri crate, build-time xtask, and one
React application.
Keep domain modules inside the core until an actual independent consumer or build
constraint justifies a crate. No crate-per-method hierarchy, inherited web server,
local Redis/MySQL, or new hosted AIRS application.

Proposed source layout:

```text
crates/desktop-core/src/{authority,runtime,gateway,academy,harness,workbench,storage}/
apps/desktop/src-tauri/       # desktop-host: native application/OS adapters/IPC
apps/desktop/src/             # React application composition
packages/desktop-contracts/   # generated Rust wire declarations + typed client
crates/xtask/                 # check/review/artifact controller, build-time only
```

The native host creates the core and its adapters once. Rust core does not depend
on Tauri, React or GPUI. The Academy-owned shared packages remain in Academy's
repository; consume reviewed immutable package/source inputs, not a mutable
absolute path or a divergent desktop copy.

## 1. Native authority interface

Choose the actor-oriented alternative described in ADR 0001. A principal-bound
Rust handle has two caller operations:

```rust
async fn dispatch(&self, intent: Intent) -> Result<Receipt, Refusal>;
fn observe(&self, scope: ViewScope) -> Observation;
```

`Intent` is a finite tagged union of domain-owned instructions, not method names
or arbitrary SDK payloads. Initial instructions are CreateTenant, SelectTenant,
ConfigureScanCredential, Scan and Cancel. Administration adds CaptureDraft,
Review and RequestApply; actual Apply is a private native-confirmation completion.
Other features add only shipped, explicitly typed instructions.
`Receipt` acknowledges admission with an opaque admitted-request reference;
operation intents also supply an operation reference. RequestApply returns
Receipt::Confirming immediately upon entering Confirming, before the native
sheet completes; the facade can admit other instructions such as Cancel or
context change. The private confirmation callback creates the actual mutation
operation atomically with review consumption, and Observation supplies its
operation reference/result. No admission call awaits a modal answer or remote
completion; a Receipt never implies remote success.

The host derives operator/Harness identity from its native invocation path.
Caller/principal/capabilities cannot be supplied by JSON. Harness has no Apply,
credential-replacement, secret-reveal or context-selection instruction. Review
references are lookup references, not bearer permission. All admission checks
repeat in Rust even for a valid reference.

Each scoped instruction carries the native-issued ContextRef that the caller
was viewing. It identifies the domain-specific scope: tenant/capability for AIRS,
workspace/grant for Gateway, issuer/learner account for Academy, or project/
conversation/grant for Harness/MCP, with applicable native generations. Academy
scope is independent of tenant selection. Rust checks it at admission and dispatch claiming. A delayed tenant-A Scan cannot
silently use the newly selected tenant B, and A→B→A never revives an old reference.
CreateTenant has a validated nonsecret profile draft and no inherited credentials;
the host creates its ID/context before separate scan-key configuration. Cancel
binds the original operation and principal rather than whichever tenant is current.

The wire envelope carries a native-issued delivery-session nonce and a string-
encoded u64 sequence. This is transport deduplication, not authorization or a
ContextRef. The typed facade serializes admission calls (not operation completion)
and advances sequence after the admission result. Native admission maintains an
O(1) high-water mark per principal/delivery session, retains all unsettled receipts
within queue bounds and the latest 256 settled admission results/input digests.
An identical retained duplicate returns the same result; changed input is refused.
An older evicted sequence is AlreadyProcessed with no dispatch, never a fresh
request. Gaps are refused without advancing. Refusals consume their accepted
sequence too, so ambiguous delivery cannot retry against changed authority.

At sequence exhaustion, explicit delivery-session rotation supplies a fresh nonce;
old sessions are retired and cannot dispatch. Renderer reload similarly registers
one new session on its native handle, retiring the old session; existing native
operations continue and are observable by authorized operation reference. This
never replays business work. A lost admission result is recovered by the same
sequence, or by observing native state if evicted, not automatic Send with a new
sequence. Explicit business retry requires new admission and operator action.
C02 tests eviction, gaps, changed duplicates, max-sequence rotation, reload with
unsettled work, bounded memory and delayed messages from retired sessions.

`Observation` atomically registers a subscriber and supplies the current redacted
snapshot. Updates are ordered, bounded and scoped. Reconnection/overflow produces
a fresh snapshot; the typed presentation adapter handles transport sequencing,
while generation rejection remains Rust-owned. Large bodies/history use bounded
paged reads or native file handles, not whole-history snapshots. Closing a view
subscription does not silently cancel a remote operation.

ViewScope is intersected with native principal authority for tenant, learner
account, project, conversation and grant. This applies to initial snapshots,
buffered updates, paged reads and payload references; a Harness handle cannot
observe operator-wide work or another learner account. Grant/context retirement
retires those subscriptions/references and discards buffered protected output.

One actor serializes authoritative transitions, without awaiting remote IO or
Keychain calls while holding that transition lock. Admitted tasks run on Tauri's
single Tokio runtime; blocking OS calls use its blocking pool. No runtime per
request. Adapter completions return as private typed messages, not callable IPC.
Tauri documents its singleton runtime and async command/channel support:
[async runtime](https://docs.rs/tauri/2.12.4/tauri/async_runtime/),
[commands/channels](https://v2.tauri.app/develop/calling-rust/).

Vault reads/writes/refresh/reveal return generation/lease-bound private messages.
Use immutable credential-record IDs; a late completion cannot overwrite a newer
record, reactivate a retired grant or reveal its secret. Zeroize stale read/token
results. A stale successful write is persisted work, not NotSent: record native
orphan-cleanup intent for that v2 record and never attach it to another context.
Credential replacement retires old authority before asynchronous storage and
blocks that capability until settlement; failure exposes an explicit recovery
choice, never silent fallback. Same-identity token refresh rotates storage
material under a stable grant, not the grant epoch; see Harness continuity below. Reset uses the same store lease and durable cleanup
ledger; no renderer-supplied account name can erase a v1 or foreign entry.

The operation state machine is Queued → DispatchClaimed → Settled. Context
retirement, cancellation and dispatch claiming share native synchronization.
Queued cancelled/retired work is NotSent. Once dispatch may begin, cancellation
or timeout alone yields Unknown; reliable rejection/completion evidence can
establish a stronger outcome. Terminal settlement is single-use; later evidence
may refine the originating protected operation record through explicit
reconciliation, never restore old presentation/review authority.

Retain per-source read supersession, interactive-before-read/FIFO priority,
interactive deadlines from submission and per-product rate-limit containment.
Separate read/review/write/stream domains; a background read cannot retire a write.
Native concurrency/queue numbers are explicit configuration qualified in C02/C03,
not references to the old worker-process constant. Start with four admitted
remote tasks and 64 queued tasks, bounded also by 64MiB of queued input; record
measured changes instead of inheriting the worker constant. A full queue refuses admission
without dropping an accepted operation. No reconnect/restart automatically replays
scans, writes, inference, grading, generic HTTP or tool calls.

Refusals are typed and sanitized: StaleContext, CapabilityMissing,
InvalidInput, ReviewInvalid, ReviewConsumed, Busy, LockedStore, DeniedStore,
AlreadyProcessed, SequenceGap and ShuttingDown.
Remote outcome remains NotSent, Rejected, Completed or Unknown, separately from
local stop reason and freshness/completeness. Arbitrary HTTP 5xx is not proof
that a mutation was rejected without effects. Unknown SDK enums remain unknown.

### C03 implementation: the first synchronous scan

Implemented in `crates/desktop-core` (`scanning`, authority `Scan` /
`CancelScan`), `packages/desktop-contracts` (validators) and
`apps/desktop/src/features/runtime`.

- **Input.** `Scan { context, draft }` carries an explicit AI security
  profile (by name, at most 100 UTF-16 units on one line, or by UUID), the
  prompt and/or response and an optional application name (at most 256
  characters), exactly v1's four inputs. Working v1 (74754c02, read-only)
  caps a scan at 96 KiB of prompt plus response (`bridge/src/runtime.ts:193`,
  `src/input.rs:396,554`) and has no context, file or other large-input path,
  so the registered limits (2 MiB per text; escaped request plus a 16 KiB
  envelope allowance within the 8 MiB SDK request ceiling) cover every
  working v1 input without staged payloads. Admission re-validates and
  refuses `InvalidInput`; the presentation checks the same limits first. The
  React form is uncontrolled: values are read from the DOM when Scan is
  pressed, never pre-captured or stored.
- **Admission** requires the current context and a ready scan credential
  (`CapabilityMissing` otherwise), then claims an operation (4 active, 64
  queued, 64 MiB) whose receipt names the `OperationId`. Work runs off the
  lock: the key is read from the Keychain on a blocking thread (a locked or
  denied key settles `NotSent` with a credential failure), the SDK client is
  bound to the credential generation's `Session` and exact origin, and the
  request carries `RequestOptions` with the operation's cancel token, the
  remaining interactive budget and `with_retries(0)`.
- **Outcomes** preserve the SDK's `Error::remote_outcome`: a parsed 2xx is
  `Completed`; 4xx and refused redirects are `Rejected`; 5xx, transport
  errors (including connect failure and a closed connection), timeouts and
  unreadable 2xx bodies are `Unknown`; `NotSent` only where the SDK or the
  desktop knows nothing was sent. The failure kind (401, 403, 429, network,
  malformed response, …) is shown separately; nothing is retried or replayed.
  Cancelling a queued scan settles `NotSent`; cancelling a running one stops
  waiting and settles `Unknown` (the request may have been processed).
- **Verdicts** are the desktop's reading of a completed scan: `block` →
  Block; `allow` + `benign` → Allow; `allow` + `malicious` → Alert; anything
  else, a reported timeout, the global error flag or any content-level error
  entry (whatever the global flags say) → Unknown, which is never presented
  as allowed; content errors are always shown. Action, category, IDs and
  detector flags (including unknown detector names) are shown verbatim,
  truncated to 512 characters; no server message text is shown. Operator
  text for an Unknown outcome never claims the request was sent: a connect
  failure or a cancellation may or may not have reached the service.
- **Context retirement.** Each scan record is bound to its tenant and
  context. A reply that settles after its context was retired (tenant
  switch, key or destination change, reset, policy revision) is discarded
  and the record says so; snapshots list only the active tenant's records
  (newest five settled per tenant plus any in flight). Records live in
  memory only: nothing about a scan is persisted, and a restart neither
  shows nor resubmits earlier scans. Retrying is pressing Scan again.
- **Fixture endpoint.** Fixture builds may trust one extra PEM root from
  `fixture-transport-roots.pem` in the fixture data root, so native
  acceptance can reach the synthetic localhost TLS endpoint; production
  builds have no such file read and trust only the OS store.
- **Budgets** are registered in `docs/architecture/budgets.json` (hash-stamped
  in `docs/planning/evidence/C03/budget-registration.json`) before any
  acceptance measurement; `xtask native` recomputes each statistic from the
  raw samples in `perf.json` (tools/perf) and `footprint.json` (`xtask
  footprint`) and fails missing, retroactive or mismatched evidence. Version
  3 changed only the scan workload's input method (paste-like insertion)
  after the version 2 run failed on memory driven by its own per-keystroke
  typing; the failing result, the diagnosis and the rationale are retained
  (`perf-diagnosis/`). Long typed input under an active assistive client
  raises WebKit's UI-process memory (about 270 MiB for 100 KiB typed); C24
  owns measuring and budgeting it.

Working one-time key reveal/copy is an explicit exception to redacted product
presentation, not a renderer credential-read method. Rust retains the generated
secret under an operator-only scoped receipt. RequestReveal opens a native
operator dialog; plaintext never enters React snapshots, subscriptions, reviews,
Harness output or diagnostics. Revalidate scope before reveal/copy; expire on
context change/dialog close, or 60s after first reveal (not key issue). Before
reveal, the receipt retains the protected secret for the current live operator
scope; app exit loses unpersisted one-time material and requires explicit key
recovery/rotation. Copy starts its own 60s clipboard-cleanup timer. Clipboard
copy is native and cleared only while its ownership marker still matches. Test this transient
path separately from the public DTO no-secret assertions; no old stored credential
can be fetched through it.

For the first scan, use immutable SDK pin
ec6230d2a19b9c835c9e4c9487795b38ad538af3 (superseded by D027: the published
`scm-sdk = "=0.1.0"` crate built from 4f65dc48, a descendant). Its Scanner::sync_scan supplies
POST /v1/scan/sync/request; its ClientBuilder accepts injected Transport,
destination policy, Limits and Session. Create an SDK Session per native
credential/context generation and cancel it on retirement; do not reuse a
cancelled Session for a replacement context. Each operation also owns its
RequestOptions cancellation/deadline, with the remaining interactive budget
measured from submission. Explicitly disable business-operation retries with
RequestOptions::with_retries(0); do not infer no replay merely because the
ReqwestTransport itself disables retries. Preserve SDK Error::remote_outcome
and distinguish application Completed success from the SDK's error outcomes.
Pool transport connections only under the policy below; clients and credentials
remain generation-scoped. Explicit first-scan Limits are 8MiB serialized request,
matching the pinned SDK default, 2MiB per prompt/response text and a 16MiB decoded response ceiling and a 60s interactive deadline measured from
submission (C03 qualifies working-v1 compatibility before acceptance). SDK content's nominal 100MiB context
limit does not override that 8MiB effective request ceiling. C03 checks v1's
actual large-context behavior; if working v1 inputs require more, its acceptance
must qualify and record a larger Limits value and native payload staging before
claiming parity. The 32MiB JSON envelope accounts for escaping, not extra SDK
capacity; larger context/files use bounded staged payload references. No silent
truncation. This source integration is designed, not natively compiled/live qualified.

### Native transport policy

Rust owns a versioned NativeTransportPolicy for each admitted credential/destination.
Use reqwest 0.13.5 with Rustls/platform certificate verification across SDK,
identity and workbench adapters. Managed SDK/identity default to Direct via
ClientBuilder::no_proxy(); no ambient macOS/env proxy is silently adopted. Explicit
proxy and System modes are operator-reviewed opt-ins, bound to policy revision.
System mode uses reqwest's macOS settings and HTTP_PROXY/HTTPS_PROXY/ALL_PROXY
behavior. Choose rebuild-per-admission: build a fresh reqwest client when each
System-mode operation is admitted to dispatch, consulting current macOS proxy
settings and the app's allowlisted proxy environment; never pool a System client
across operations. No SystemConfiguration observer or promise to retire in-flight
requests on an unobserved OS change is assumed. Changes apply to the next
admission, tested by changing the synthetic proxy configuration between requests.
Explicit app policy/root revisions do retire applicable authority. Generic
workbench defaults to System for HTTP-client parity, clearly visible in settings;
managed workbench uses the managed policy. There is no automatic direct/proxy
fallback. Explicit proxy authentication stays native and is never placed in logs.

Default trust is the OS platform trust store through rustls-platform-verifier;
additional PEM roots are an explicit reviewed per-policy input with content hashes.
Do not disable certificate/hostname validation or emit TLS key logs. A trusted
enterprise interception root may authorize interception under the OS policy; do
not claim TLS defeats an operator-installed trust root. Destination admission
continues to bind the origin URL/token endpoint independently of the network
proxy. Policy/root revisions retire clients, reviews and applicable ContextRefs;
no credential-bearing redirect changes the approved destination. Inject an SDK
Transport using the configured client rather than relying on SDK defaults.
This is an app-owned impl scm_sdk::Transport; the pinned ReqwestTransport cannot
accept this configured client. Depend on scm-sdk with default-features=false,
features=["scan"] for C02/C03, adding only accepted later families. Preserve the
Transport contract: exact admitted request body, TLS, no redirects or reqwest
retries, bounded headers/chunks, gzip-only decompression with limits applied to
decoded bytes, sanitized errors and resource release when future/body drops.
Native adapter fixtures verify those obligations independently of SDK builders.
OIDC/OAuth disable default HTTP features and use a custom AsyncHttpClient adapter
backed by this reqwest version, avoiding an accidental second reqwest major.
C02/C15 qualify direct/system/explicit, env/macOS proxy changes, extra roots,
wrong-host/untrusted certificates, redirects and cancellation using synthetic
native endpoints; C04/C20/C22 repeat identity-specific destination cases.

### C02 implementation: contexts, credentials and transport

Implemented in `crates/desktop-core` (authority, `tenants`, `context`,
`vault`, `store`, `runtime`, `transport`, `scan`, `import`) and
`apps/desktop/src-tauri` (`keychain`, `adapters`).

- **Tenant profiles** carry only a native-issued ID (`tn-` + 16 hex), an
  operator label, an optional TSG ID and a scan destination: one of the four
  SDK regions or a custom HTTPS origin (host required; no userinfo, path,
  query or fragment; normalized to its ASCII origin). IDs deserialize only in
  the issued shape.
- **Contexts.** A `ContextRef` (`cx-` + 24 hex) is issued from a monotonic
  epoch on every selection, credential replacement/removal, destination
  change, transport-policy revision and reset; A→B→A issues a third value.
  Tenant-scoped intents carry it and are refused `StaleContext` otherwise.
  Reissue retires every operation bound to the previous reference (queued →
  NotSent/ContextRetired; claimed → cancelled) and drops a pending endpoint
  review bound to it.
- **Scan credentials** are immutable records (`cr-` + 16 hex) bound to a
  tenant and to the destination origin at the time of entry, stored in the
  Keychain or, by explicit choice, session-only in memory (never persisted,
  zeroized on removal and exit). Entry is `ConfigureScanCredential { context,
  mode, key }`: the key is validated as an SDK scan header, the previous
  record is retired first, the new record is written off the transition lock
  under a lease, and only a completion holding the current lease attaches it.
  Keychain writes are write-ahead: the record joins the durable cleanup
  ledger before the write and leaves it in the same atomic save that commits
  it, so a crash never leaves an untracked item. The secret is written only
  after that write-ahead save is confirmed durable (one confirming retry); if
  the directory sync stays unconfirmed, nothing is written and the capability
  fails visibly (`unavailable`) for the operator to enter the key again; a stale successful write is
  deleted, never attached elsewhere. Locked/denied/unavailable stores fail
  the capability explicitly (`Failed { failure }`) with operator recovery
  (enter again, or choose session-only); there is no plaintext or session
  fallback. Delivery digests are salted per launch and the serialized intent
  is zeroized, so no unsalted key hash is retained.
- **Endpoint overrides** are reviewed: `ReviewEndpointChange` stores a review
  (from/to origin, whether the key is removed) that the presentation shows;
  `ConfirmEndpointChange` must present the same context and review. Confirming
  retires the context and the current scan key: an existing credential is
  never re-bound or forwarded to a new destination; the operator enters a key
  for the new destination. The SDK client admits exactly the bound origin, so
  a redirect or rebinding to another origin is refused before any request
  carries the key.
- **Profile import** opens the native open sheet attached to the main window
  (rfd 0.17.2); the host reads at most 1 MiB from a regular file off the main
  thread and passes only the bytes and base name to the core. The core
  previews the v2 profile document (`prisma-airs-desktop-v2/tenant-profiles/1`)
  or the airs-cli tenant registry, keeping label, TSG ID and destination
  only, counting discarded secret fields, file references and other fields,
  and never opening a referenced file. `ConfirmProfileImport` creates new,
  keyless tenants from the explicitly selected valid entries; existing
  tenants are never changed. File contents never reach the renderer.
- **Storage.** Nonsecret state (appearance, tenants, active tenant, Keychain
  index, cleanup ledger) is one versioned JSON document in the bundle
  identifier's application-data directory: owner-only 0700/0600, atomic
  replacement with fsync, `O_NOFOLLOW`, and an exclusive lock file held by
  the running app. A failure before the rename changed nothing: memory is
  restored and the intent is refused. A failure after the rename (the
  directory sync) means the new document is in place but its durability is
  unconfirmed: the change is kept, admitted and reported
  (`durabilityUnconfirmed`) until a later save confirms; it is never rolled
  back in memory while present on disk. A corrupt or inconsistent document
  is reported (`storage: corrupt`) and left untouched; persisting intents
  refuse `DeniedStore` until the operator resets. Appearance now persists
  (C01 carry-over).
- **Reset** (`ResetLocalData`) is ordered and durable. Admission retires all
  authority at once (every context reference and operation, session
  secrets, every credential binding, with Keychain records moved into the
  cleanup ledger) and saves that retirement before anything is deleted; an
  unavailable store (possibly another instance's) refuses. The reset then
  waits until every admitted Keychain write has settled (no new write can
  start while resetting), purges only this build's v2 namespace
  (`io.cdot.prisma-airs-desktop.v2.scan`; fixture
  `io.cdot.prisma-airs-desktop.v2.fixture.scan`) and removes the document. A
  stop is reported by stage (`credentials` or `document`) with keys already
  out of use and the ledger finishing deletion later (also after a crash or
  restart); a fresh context is issued and no earlier reference revives.
- **Keychain adapter.** Classic generic-password items (service = namespace,
  account = record ID) via security-framework 3.7.0, every call serialized.
  Production uses the login Keychain with OS prompts allowed; fixture builds
  use a dedicated file keychain in the fixture data root with prompts
  disabled. OS codes map to sanitized errors; a locked keychain is told apart
  from an ACL denial by reading its lock state after the prompt guard is
  released (macOS 27 answers -25293 for an open keychain locked externally).
  `SecKeychainItemDelete` and `SecKeychainGetStatus` are called directly
  because the crate drops the delete status and does not bind the status call.
- **Operations runtime.** Queued → DispatchClaimed → Settled with 4 active,
  64 queued and 64 MiB of queued input, an interactive deadline measured from
  submission, single-use settlement and no retry or replay. C03's Scan intent
  admits through it.
- **Transport.** `NativeTransport` implements `scm_sdk::transport::Transport`
  over reqwest 0.13.5 (rustls, platform verifier): redirects and retries off,
  gzip only (SDK limits apply to decoded bytes), at most 128 response headers
  and 64 KiB of header bytes, decoded chunks split to 1 MiB, sanitized
  errors, connection released on drop. Direct (the default) ignores ambient
  proxies; System builds a fresh client per admission from the proxy
  environment and macOS HTTP/HTTPS proxy settings (no PAC/auto-discovery or
  bypass list, which reqwest does not read); Explicit uses the reviewed proxy.
  Extra roots must each parse as X.509 trust anchors (reqwest would otherwise
  accept an unparseable root lazily). A policy revision retires contexts.
  C02 has no operator UI for proxy policy; C15 adds it.
- **SDK binding.** `ScanBinding` builds an SDK client per credential
  generation with its own `Session`, an exact-origin `OriginPolicy`, the
  first-scan limits (8 MiB request, 16 MiB decoded response, 60 s, no
  retries) and per-request `with_retries(0)`.

## Fresh editor input and Configuration review

React owns immediate editor text/undo; Rust owns captured immutable drafts and
review validity. Do not round-trip every keystroke through Rust to redraw text.
A fresh full editor snapshot is captured synchronously in the production Review,
Apply, Save and Send handlers. It carries raw validity, document identity and
input content; a debounced cached revision alone is insufficient. Invalid or
lossy input is refused without overwriting controls or claiming a faithful save.

Review captures/validates/normalizes the fresh draft in Rust and stores an opaque
review record bound to application session, tenant, credential revisions,
Gateway workspace if relevant, approved destination, resource/version, exact
normalized draft/action and expiry. Apply receives the review reference plus a
fresh editor snapshot for comparison. Rust constructs the outgoing operation
only from the stored reviewed draft, never replacement client payloads. Different
current input, invalid input, retired context/resource or expired review refuses
Apply. Review consumption and operation creation are atomic; duplicate delivery
returns the same operation only for the same native-bound invocation and input.
A different invocation cannot reuse the consumed review, even after failure.

RequestApply validates the fresh snapshot and opens a native operator confirmation
showing Rust's stored review summary/target. Its accept/deny callback is private
to the native host, session/review-bound and never serialized as a renderer
approval boolean/token. Recheck bindings after confirmation and before dispatch;
context changes while the dialog is open invalidate it. The host actively ends
the NSWindow sheet on retirement, not only when its callback arrives. First-reveal
expiry also ends the secret sheet, clears its plaintext accessory and zeroizes
the owned secret buffer; C09 asserts dismissal/clear on both paths. This makes actual Apply
depend on operator confirmation beyond possession of a main-window IPC channel.
Harness cannot request this dialog or manufacture its completion. Native tests
cover decline, forged callback/reference, changed context and double confirmation.

Use a Rust-side AppKit NSAlert sheet attached to the actual main NSWindow through
objc2-app-kit 0.3.2, objc2 0.6.3, objc2-foundation 0.3.2 and block2 0.6.2.
Construct/present on the main thread; beginSheetModalForWindow completion returns
a private nonce-bound actor message. It is never a second WebView or JS dialog.
The same native adapter supplies the transient one-time-secret accessory view;
never serialize its contents back to React. NSPasteboard writes a random custom
owner-nonce type plus the secret, recording changeCount after the write. Also
set org.nspasteboard.ConcealedType and org.nspasteboard.TransientType so cooperating
clipboard managers can omit the secret; these are hints, not enforced secrecy. Cleanup
checks both ownership nonce and changeCount immediately before clearing; skip if
another owner changed it. macOS offers no atomic clipboard compare-and-swap, so
this is best-effort cleanup, not a guarantee against external clipboard races or
clipboard managers. C09 tests ownership changes and expiry using native fixtures.

The review states are Reviewed → Confirming(native invocation) → Consumed(operation).
Duplicate delivery reuses its pending dialog/result; a competing invocation is
refused while confirming. Denial retires that confirmation nonce without sending;
another attempt requires fresh capture/confirmation. Late/duplicate callbacks
cannot consume it again. Confirmation holds a modal editor/document lease: main
input, navigation and document replacement are disabled; a captured/programmatic
draft change invalidates the lease. Revalidate at callback. Native tests exercise
pending-dialog edits, not only typing before RequestApply. The native dialog's
stored operation summary remains the explicit authoritative action shown to the
operator; no claim is made that Rust can inspect arbitrary renderer-only text.

Re-read the target baseline before a reviewed mutation and use conditional
resource/version preconditions where the actual endpoint supports them. Without
server-side compare-and-set, an external writer can still race the final read;
do not promise atomic remote conflict prevention. Preserve that limitation,
readback/reconciliation and exact known/unknown outcome semantics.

Typing after Review makes it obsolete for the new input. Fresh capture at Apply
catches changes that have not reached a debounce or host capture; production
UI tests must exercise that exact race. Rust cannot independently prove what a
compromised renderer visually displayed: window admission and content binding
are enforced native rules, while faithful user-input capture is a tested
presentation contract. A renderer-supplied 'operator: true' is never sufficient.
Harness proposals enter captured drafts but cannot provide operator Apply.

## Tauri contract and admission

Use named host commands `desktop_dispatch` and `desktop_observe`, with finite
versioned wire unions; no generic shell/filesystem/HTTP/Keychain plugin grants.
Only the packaged main WebView may call them. Commands validate the native
window identity and allowed local origin as well as schema/version/size limits;
capabilities alone do not authorize a domain operation. External OAuth opens in
the OS browser, not a privileged remote-content WebView. Navigation to remote
content is denied; content/downloads are treated as data.

Explicitly register the two application commands in Tauri's application command
permission manifest; grant only the packaged main window. Do not assume custom
commands inherit restrictive plugin defaults. Validate domain intent again in
the host, and keep native completion/fixture commands absent from that grant.
The initial JSON envelope ceiling is 32MiB so normal SDK prompt/response limits
are not silently reduced by IPC escaping; bound queue bytes independently.
Large context/files use native-owned payload references selected/staged through
scoped intents, never renderer-supplied arbitrary filesystem paths. The host
checks reference owner, content length/hash and SDK limits at admission. Page
large projections; bound each observer to 256 queued notifications and resync
from native state on overflow rather than retaining unbounded history.

Generate TypeScript declarations from Rust DTOs using ts-rs 12.0.1's documented
Serde support. Commit generated output and check regeneration diff. Use tagged
unions and string-encoded opaque revisions/IDs; do not serialize u64 as unsafe
JavaScript numbers or assume generated declarations perform runtime validation.
Rust rejects unknown fields/variants and malformed/oversized requests. Literal
serialization fixtures test the contract, including no secret fields. The small
TypeScript invoke facade is tested against those fixtures. See
[ts-rs documentation](https://docs.rs/ts-rs/12.0.1/ts_rs/).

Production CSP permits packaged assets and necessary local Tauri transport;
remote product HTTP runs in Rust. No remote scripts, unsafe-eval or broad
connect-src fallback. Academy HTML/Markdown uses its reviewed safe rendering
path; no HTML can acquire native command authority. Development-only fixture
endpoints/loopback permissions are excluded from production configuration.

## 2. Academy-owned shared presentation

Keep the present `@ai-security-academy/ui` tooling entry compatible. Introduce
explicit shared package exports for controls, tokens/themes and a provider-free
shell; do not export the web authenticated composition through that surface.
Move the implementation once and make Academy web consume it too. C01 extracts
only this general presentation, with regression checks in both consumers. Align
@ai-security-academy/ui React peer declarations with the consumed 19.2.3 root
version during extraction; preserve a single React instance per consumer.

The shell interface supplies `children`, brand, navigation items/active state,
header/breadcrumbs, account/actions, command palette, and responsive navigation
slots. The shared implementation owns semantics/layout/focus and visual tokens.
Web composition supplies Clerk-aware navigation, catalog search, notifications
and learner actions. Desktop composition supplies local feature navigation,
tenant controls, native shortcuts and app preferences. No universal plugin system.

Theme rendering is shared, persistence is an adapter: Academy web continues its
hosted learner preference; desktop global theme is an app preference persisted
in Rust. C01 keeps it in core memory (C01 writes no user data); C02's
protected application storage persists it, with a restart test. Academy account appearance may inform its feature composition without
silently writing a learner preference on every desktop theme toggle. Gamification
providers/effects mount only in the Academy feature and retire on account change.

C05/C06 extract learner presentation into an Academy-owned learner export with
an explicit navigation/transport provider. Preserve existing React Query/tRPC
hooks where possible; move type-only learner contracts into an independently
buildable package rather than importing the Express/DB app to generate desktop
types. The desktop transport is a terminating tRPC link that calls Rust; web
uses its existing authenticated HTTP link. tRPC supports custom terminating
links: [official link documentation](https://trpc.io/docs/client/links).

The Rust Academy adapter accepts only a revision-pinned allowlist of learner
procedures and query/mutation kinds, including the legitimate learner feedback
procedure despite its admin namespace. It constructs the approved hosted URL,
injects native account credentials and enforces size/time/cancellation policy.
It cannot execute arbitrary renderer URLs or manager procedures. Independent
wire fixtures cover SuperJSON representations, tRPC envelopes/errors and
version mismatch. Never claim every learner hook portable until both consumers
build and the mapped procedure set passes.

Hosted desktop access/projection, grading, exam timing and awards remain
unchanged. Use existing desktop bundle/attempt/draft contracts for offline work;
raw source course content, answer keys/private evaluation and assessor credentials
never enter the desktop build. Account-specific QueryClient state is retired
on sign-out/switch; tenant switching does not change Academy account identity.

### C04 implementation: Academy identity and learner transport

Implemented in `crates/desktop-core` (`academy`, `learner`,
`authority/learner_domain`), `apps/desktop/src-tauri` (Academy Keychain
namespace, browser opener) and `apps/desktop/src/features/academy`.

- **Device sign-in** (D028). `StartAcademySignIn` reads Academy's public
  `/api/desktop/config` (issuer must be a bare HTTPS origin; client ID
  `[A-Za-z0-9_-]{1,256}`; scope containing `academy:learn` and
  `offline_access`), the issuer's RFC 8414 metadata (its `issuer` must equal
  the configured one and every endpoint must be HTTPS on that origin), then
  starts an RFC 8628 device authorization as a public client (no secret).
  Polling honors `interval`, adds 5 s on `slow_down` and stops on
  `access_denied`, `expired_token` or local expiry. A grant must be Bearer,
  carry `academy:learn` when a scope is returned and include a refresh token;
  the account is bound through `desktop.me` (`user_…` ID, display name).
  Flows are numbered: cancel, a new start, sign-out or reset supersede a
  flow, and a grant that lands afterwards is revoked, never kept.
  Replies are bounded (128 KiB), redirects refused, transient 5xx read as
  network. Clerk/Academy text is never shown.
- **Browser hand-off.** `OpenAcademyVerification` carries no URL: the host
  opens the HTTPS page the issuer returned for the flow in progress
  (`/usr/bin/open`; the fixture build records it in its data root instead).
- **Protected session.** The refresh token is stored in the learner Keychain
  namespace (`io.cdot.prisma-airs-desktop.v2.academy`; fixture
  `.v2.fixture.academy`) under the same write-ahead discipline as scan keys:
  each record joins a durable learner cleanup ledger (confirmed save, one
  confirming retry) before it is written and is marked in flight; the
  account takes ownership in a later save that removes it from the ledger.
  Abandoned, replaced, signed-out and superseded records stay in the ledger
  and are deleted by record (never by purging the namespace outside reset),
  only when no write or revocation uses them and the document naming their
  replacement is durably confirmed; failures retry at the next launch. The
  state document keeps only account ID, display name, record, issuer and
  client ID. Access tokens live only in memory. A Keychain
  failure fails sign-in visibly (the grant is revoked); there is no
  plaintext or session fallback. On restart the saved account resumes signed
  in and renews its access token from the Keychain on first use; a missing,
  unreadable or refused credential reads as an expired session that names
  the account and offers sign-in again or sign-out.
- **Renewal** is single-flight per account generation and runs to
  completion even if its caller is cancelled, so a rotated refresh token is
  either saved (write-ahead record, account moved to it, old record ledgered
  and deleted once confirmed) or, when the account changed meanwhile or the
  save failed, revoked.
- **Learner reference.** `LearnerRef` (`lx-…`) names one account
  generation, issued on sign-in, restart and transport revision and retired
  on sign-out, account change and reset. It is independent of tenant
  contexts: the operations runtime retires contexts per domain prefix
  (`cx-` tenants, `lx-` learner, all on reset), so tenant switches never touch
  learner work.
- **Learner calls.** `AcademyCall { learner, procedure, procedureKind, input }`
  admits only the 99 procedures pinned from Academy 891f65ee/7f3a28c4
  (`LEARNER_PROCEDURES` with each router's kind; manager, assessor and admin
  procedures other than the learner `admin.feedback.submit` are refused
  before any request). Inputs stay in SuperJSON wire form (≤ 1 MiB, valid
  JSON); queries GET `/api/trpc/<p>?input=…`, mutations POST it, never
  batched, with the access token. A refused token is renewed once; a query
  is then asked again, a mutation never. Replies (≤ 16 MiB) must be tRPC
  envelopes; server-private assessment fields (`answerKey`, `serverSpec`,
  `rubric`, `goldens`, `evaluation`) are refused in results and in error
  envelopes alike, except the orientation drill's numeric practice
  `answerKey` in its result. Error results carry their remote outcome
  (Rejected for 4xx, Unknown for 5xx). Results are read once through
  `ViewScope::AcademyCall` (`desktop_observe` scope `academyCall`); a reply
  for a retired learner reference is never delivered, and settled replies an
  earlier account never observed are withdrawn on account change.
- **Presentation.** Settings holds the Academy account (code, issuer page,
  open, cancel, sign out, failure recovery). `desktopLink` is a terminating
  tRPC link that admits each operation natively and rebuilds the envelope for
  tRPC's own `transformResult`, so Academy hooks see the same values and
  errors as over HTTP; every error also carries the native remote outcome
  (`data.outcome`), and an unknown mutation (lost reply, timeout or
  cancellation after dispatch, 5xx, unconfirmed admission) is described as
  possibly applied, with a server's own message and codes kept in `data`
  (`serverMessage`) — nothing is resent; `LearnerProvider` keys React Query and tRPC clients by
  learner reference and remounts the learner subtree on account change.
  Literal SuperJSON/envelope fixtures in
  `packages/desktop-contracts/fixtures/academy` drive both the Rust and the
  TypeScript suites. Academy content arrived in C05 (below).
- **Sign-out** moves the saved record into the ledger in one save (a save
  failure refuses sign-out before anything changes), retires learner work,
  revokes the refresh token (read from the Keychain first when this launch
  never loaded it) and then lets cleanup delete that record; a newer
  sign-in's record is never touched.
- **Reset** signs the learner out, waits for admitted learner writes like
  scan writes, and purges the learner namespace with the scan namespace
  (stage `credentials`).

### C05 implementation: Academy catalog, lessons and knowledge checks

Implemented in Academy's `@ai-security-academy/learner` package (branch
`v2/desktop-presentation`, pinned in `docs/architecture/inputs.lock.json`),
`crates/desktop-core` (`learner`, `authority/learner_domain`) and
`apps/desktop/src/features/academy`.

- **Shared learner source.** The desktop compiles Academy's own learner pages
  (catalog `LearnPage`, `PathLandingPage`, `CoursePage`, `UnitPage` with
  lessons, diagrams and knowledge checks, `CompletionMoments`) from the
  pinned, hash-verified Academy input: the learner package exports them with
  `LearnerHost`, which supplies the tRPC links, the account-scoped
  QueryClient, a provider-neutral learner identity (Academy web supplies it
  from Clerk, the desktop from its native account) and the read-only theme.
  Academy-relative `@/` imports resolve to `.work/academy/src` and every
  bare import to the desktop's pinned copy (`academy-source.ts`, Vite
  `dedupe`), so one React, React Query and router exist. Public types are
  declared in the package and checked against the implementation
  (`src/conformance.ts`). Academy web keeps its routed composition. The
  hermetic shared suite (`vitest.presentation.config.ts`, run by the shared
  gate on the pinned input) covers the learner export and host and the web
  learner compositions (units, learn, library, labs). Every file-installed
  Academy package (presentation, learner, shared) is fingerprinted and
  verified against the pinned input in its staged and installed copies.
- **Guest and account generations.** A `LearnerRef` exists whenever the core
  runs: the guest generation while signed out, an account generation once
  signed in (`ShellSnapshot.learner`; `AcademyView::SignedIn.accountId`).
  `AcademyCall` admits a signed-in learner's procedures on the pinned
  account allowlist with the account credential, and the public catalog
  queries (`PUBLIC_PROCEDURES`: catalog, course, path, module and unit
  reads, as Academy's public procedures) without any credential, for guests
  and for signed-in public-only procedures; everything else is refused
  before a request, and a guest call is never renewed or resent. Each
  sign-in, sign-out, account change and reset retires the previous
  generation (`StaleContext` for its calls) and the presentation remounts
  the learner subtree with a fresh QueryClient and router.
- **Desktop learner host** (D029). The Academy destination routes Academy's
  paths in memory (`/learn`, `/paths`, `/paths/$pathSlug`,
  `/courses/$courseSlug` with Academy's retired-slug redirects,
  `/units/$unitId`, `/sign-in` pointing to Settings; C06 adds the Program,
  assessment, certificate, Shift and account routes); any other path
  (Academy's manager and admin areas) says plainly it is not in the desktop
  app, and a page that fails to render shows a plain failure view without
  its error text. Focus moves to each new page's heading. Learner pages' breadcrumb
  trail (`useLearnerPageHeader`) renders as links in the destination.
  Lesson-body anchors route learner paths in place, scroll in-page anchors,
  and leave any other address out of the window (the navigation policy
  already refuses remote pages), saying where it points. Hands-on labs are
  previewed (`labs="preview"`: title, brief, no sandbox started); lesson
  diagrams show the pipeline's pre-rendered SVG for the theme as an image
  with its source under a disclosure, or the source when no artifact
  exists (`diagrams="prerendered"`): mermaid cannot load under the
  WebView's frozen prototypes and its SVGs rely on inline styles the CSP
  refuses. Diagram walkthrough videos are not requested.
- **Truthful denials.** Academy's own content-access decisions pass through
  unchanged: a guest at gated content gets Academy's sign-in gate (401), a
  signed-in learner without the grant or progression its locked gate with
  the domain reason (403 `FORBIDDEN`), and retired content "Unit not
  found." (404). A procedure the desktop refuses reads as Academy's sign-in
  gate for a guest (`UNAUTHORIZED`: "Sign in to take this knowledge check.")
  and as not carried by the desktop for a signed-in learner
  (`METHOD_NOT_SUPPORTED`), never as Academy's grant/progression gate
  (`FORBIDDEN`); none of these is retried.
- **Learner effects stay in Academy and in their generation.** Completion
  moments, XP and gamification components mount only inside the Academy
  destination; the shell and other features mount none and make no learner
  calls. The desktop passes its learner generation as Academy's learner
  session; Academy's identity provider announces each session change before
  any page effect of the new session runs; completion moments keep only the
  current session's watch and queue and ignore late calls carrying another
  session, and toasts, the level-up overlay, the lab-pass award and
  in-progress answers are retired, so sign-out, another account, reset,
  re-sign-in to the same account (even while Academy is unmounted) or
  leaving and re-entering Academy never show an earlier generation's state.
- **Visible failures.** `LearnerHost` mounts Academy's toaster and reports
  its host QueryClient's mutation failures as Academy web does. A failed
  learner mutation whose outcome is unknown (lost reply, timeout, 5xx after
  receipt) re-reads the unit progress it would have changed and says what
  the learner can do: starting a check again is safe, completing a unit
  again is safe (idempotent), and a possibly recorded submission shows as
  Done in the contents once Academy recorded it (its score cannot be
  shown); a guest is asked to sign in (unit completion inline, knowledge
  checks, the notice); nothing is resent. Academy's
  WebView presentation keys (`academy-` completion latches, contents-rail
  preference) survive sign-out as on Academy web and are removed when a
  local-data reset completes.
- **Truth exclusion.** `tools/academy/truth-graph.mjs` builds the production
  frontend with a module-graph plugin: no Academy server, database, authored
  content or question bank, content pipeline, script, admin, Redis or
  grading/evaluation module is in the graph, no server or provider package
  is bundled, a canary seeded into each such module never appears in the
  output (a canary seeded into the learner host does, proving detection),
  and every procedure the shipped modules call is on the account allowlist,
  public, or listed as refused by design with its reason (lab runs —
  previewed, from C06 run through the desktop attempt contract; event
  celebrations — never mounted, from C06 gated by `awardPresenters`;
  product telemetry). The C05 test gate runs it (`vitest.truth.config.ts`); its report is in
  `docs/planning/evidence/C05/truth-graph`.
- **Fixture-only measurement.** In fixture builds (`snapshot.fixtureBuild`)
  the destination offers a learner-call probe for the C04-registered
  overhead budget; production builds never render or load it.

### C06 implementation: graded workflows, Program and the learner account

Implemented in Academy's learner package and pages (branch
`v2/desktop-presentation`, pinned in `docs/architecture/inputs.lock.json`),
`crates/desktop-core` (`authority`: `PrintPage`, `PagePrinter`,
`ShellSnapshot.academy_origin`), `apps/desktop/src-tauri/src/adapters.rs`
and `apps/desktop/src/features/academy`.

- **Academy's own pages, more of them.** The learner export adds Home,
  Program, the final assessment (intro and runner), the identity practical,
  certificates (own and by id), Shift, the dossier, the weekly board,
  account settings, notifications, requisitions, orientation and onboarding,
  with Academy's navigation (`LEARNER_NAV`: Home, Learn, Program) and the
  web shell's account entries (`LEARNER_ACCOUNT_NAV`: Notifications,
  Settings; a parity test keeps both equal to Academy web's shell). Guests
  see Learn only; an account route asks a guest to sign in and sends
  nothing. Learner identity carries the native account's display name
  (from `desktop.me`) as its profile, never an identity-provider session;
  email and other account facts come from Academy's own settings read.
  The page's session-critical status (`useLearnerPageHeader().status`: the
  final assessment's timer, which submits once at Academy's deadline, and
  its proctored pill) renders in a sticky slot under the trail and unmounts
  with the attempt or the learner generation.
- **Hosted authority is passed through, not recomputed.** Exam, Shift,
  practical, lab and draft requests travel exactly as the page wrote them;
  Academy's deadlines, SLA stamps, grades, awards, submission state and
  review status come back untouched and are what the pages show. The core
  sends each learner mutation once: a 5xx or lost reply is an unknown
  outcome (`Outcome::Unknown`), the page says Academy may have recorded it
  and offers Academy's own status read (`exam.submissionState`, the
  attempt/draft reads) before any retry, and a retry the learner chooses
  reuses the page's idempotency key (submission id, mutation id, request
  id). A changed draft version is Academy's `CONFLICT`, shown with both
  copies until the learner keeps one. Assessor, grading, override, award
  and admin procedures are not on the allowlist and are refused before any
  request.
- **Timed work is never held by optional context.** Calls are independent
  operations: a slow `shift.briefing`/`shift.dailyGhost` or debrief never
  delays `shift.serve`/`shift.answer`/`shift.decide` (fixture-tested with a
  delayed briefing), and a stale unauthorized reply of an earlier
  generation is cancelled with that generation, so it cannot retire a newer
  account.
- **Host capabilities (D030).** `LearnerHost` gains `labs: 'desktop'`
  (guided labs through Academy's `desktop.attempt`/`desktop.draft`
  contract: start once per request id, versioned draft saves with mutation
  ids, product-sandbox operations applied and replayed client-side, grading
  only on a confirmed draft version with a submission id, conflicts
  resolved by the learner, unsupported labs previewed, locked attempts
  explained), `webOrigin` (public verify links on Academy web's origin from
  `ShellSnapshot.academyOrigin`, validated as an HTTPS origin), `onSignOut`
  (the account page signs out through `SignOutAcademy`), `onPrint`
  (certificate download opens the native print sheet through `PrintPage`,
  observed like a learner call: a sheet that cannot open reports
  `hostUnavailable` and the page says so; nothing changes either way; the
  fixture build records it), `courseReset` and `lessonFeedback` (retained
  from v1: Academy's reset confirmation with its counts and kept
  certificates, and a completed lesson's rating through
  `admin.feedback.submit`; both off on Academy web), `onUploadAvatar` and
  `loadImage` with `remoteImages=false` (below), `appearance='host'`
  (theme and accent follow the desktop), `assessorTools=false` (the
  practical's assessor queue and review stay on Academy web and are never
  fetched) and `awardPresenters=false` (Home does not claim away
  commendations or event celebrations; Academy web announces them and the
  dossier lists every commendation). Academy web keeps every default.
- **Avatar outside the renderer.** `UploadAvatar` opens the native image
  picker (fixture builds use a staged file) for a source photo of up to
  16 MiB, decodes it in the core within bounds (JPEG, PNG or WebP
  recognised from its bytes; at most 8192 pixels a side and 128 MiB of
  decoder memory, the decoded pixels and then the resampling buffers
  reserved from that budget before they are allocated), resamples only the
  centred square (read in place, so an extreme aspect ratio never scales up
  the whole image) to 512 pixels, applies its EXIF orientation to that
  square (a centred crop commutes with every rotation and flip, so no
  full-size copy) and re-encodes it as a JPEG within Academy's 2 MiB cap (as v1 did with the same `image`
  0.25.10 decoders); anything that does not decode completely is never
  uploaded. It then presigns with the account credential
  (`settings.avatarUploadUrl`, which changes nothing and may be asked
  again after a renewed token), stores the JPEG with one credential-free
  PUT only at a reviewed destination (`crate::avatar::upload_destination`:
  HTTPS, signed query, the `*.r2.cloudflarestorage.com` object store or
  Academy's own origin), and confirms with Academy
  (`settings.confirmAvatar`, never resent). Academy keys avatars by user,
  so a stored PUT can replace the photo already served at the same address
  before or without confirmation: only a refused (4xx) or unsent PUT
  reports the photo unchanged; a lost or failed store reply, cancellation
  after sending, and any confirmation that is not a success report that the
  photo may have changed (`Outcome::Unknown`), and the page re-reads
  Academy's settings and the image at its (possibly unchanged) address.
  `LoadAcademyImage` reads only Academy's public avatar address
  (`/api/avatars/<id>.<ext>`, no credential) and hands the page a `data:`
  JPEG re-encoded the same way, only when the declared type matches the
  decoded image. Removal is Academy's `settings.removeAvatar`. Both jobs run
  in the learner generation and are retired with it; the picker wait has
  its own budget (`OperationsRuntime::admit_for`), each request its own
  timeout.
- **Lab drafts are serialized.** Academy's desktop draft saves run one at
  a time against the work current when each starts; an older reply never
  replaces a newer confirmed version or clears another save, and a queued
  save of confirmed work sends nothing.
- **WebView constraints met in Academy, not loosened here.** Academy web's
  lab runtimes (editors, drawing canvas, renderers) load lazily only for
  `labs='run'`, and the diagram-label sanitizer uses DOMPurify instead of
  sanitize-html, whose date dependency assigns over frozen built-ins (the
  JavaScript override mistake) and could not load under `freezePrototype`.
  Award-toast bookkeeping (beacon, acknowledge) is silent like Academy's
  other best-effort writes, so a refused beacon never surfaces as a failure.
- **Course removal and reset.** Removing an enrollment shows Academy's
  confirmation naming every affected certification and its courses and
  that earned certificates stay valid, and sends one
  `enrollment.removeByCourse`. Course reset (retained from v1, though
  Academy web moved its trigger to an admin panel) shows Academy's
  confirmation with the course's unit, quiz and lab counts and that earned
  certificates stay valid, and sends one `progress.resetCourse`.
- **Route sweep.** `learner-routes.test.tsx` renders every desktop learner
  route against a core that admits exactly the allowlist and public
  queries: no route may show an error toast, and every refusal must be one
  the truth graph lists with a reason.

### C07 implementation: encrypted offline work

Implemented in `crates/desktop-core` (`offline`: the sealed store;
`authority/offline_domain`: downloads, offline answers, revalidation and
lab work kept on this Mac; intents `DownloadCourse`, `RemoveDownload`,
`SaveLocalDraft`, `LoadLocalDraft`, `ClearLocalDraft`;
`ShellSnapshot.academyOffline`), `apps/desktop/src/features/academy`
(Downloads page, offline notice, `nativeLocalDrafts`) and Academy's
desktop draft hook (`LearnerHost localDrafts`, pinned in
`docs/architecture/inputs.lock.json`).

- **Sealed per account (D031).** Each signed-in account has a directory
  under the app data root named by a digest of its account id and its own
  random 32-byte key in the learner Keychain namespace (record
  `cr-<digest>`, deterministic per account, created on the first download
  or the first lab work kept on this Mac, never before; sign-out keeps it
  with the locked work, reset removes it). Every record is XChaCha20-Poly1305 sealed
  under a fresh nonce with associated data binding the schema, the account
  digest and the record's file name, so no record reads as another
  account's or another record. Writes are atomic (a temporary file of the
  writer's own, sync, rename, directory sync; leftovers of an interrupted
  write are removed when the store opens), directories 0700 and files
  0600, one instance per account (an exclusive lock). Within the instance,
  the quota check, every commit, deletion and set-aside runs under one
  commit lock, and read-modify-write goes through `OfflineStore::update`.
  Bounds: 16 MiB per record (the learner reply cap), 512 MiB and 1024
  records per account, counting records kept aside; beyond them a download
  is refused (`offlineFull`) and nothing is kept. A record that does not
  authenticate (re-checked under the commit lock) is renamed aside
  (`.unreadable-<time>`) and kept, the newest eight; the rest still opens.
  The key is proven against a sealed key check (or, for a store without
  one, a record that authenticates) before anything is read, set aside or
  written; a store that cannot open (Keychain denied or locked, a key that
  cannot be proven, another instance) stays closed and touches nothing
  (`offlineUnavailable`), and the right key recovers everything. There is
  no plaintext path.
- **One write ledger with reset.** Every offline store operation, key
  creation included, first enters its generation's ledger
  (`OfflineGen::enter`): a retired generation (sign-out, account change, a
  new generation) or a reset in progress admits none, and reset waits for
  every admitted one before purging the Keychain and the offline root, so
  nothing is written or recreated after reset completes. Downloads leave
  the ledger while Academy answers and enter it again to save.
- **What is kept.** A course is downloaded only after `desktop.access`
  says the account may have it, as Academy's `desktop.bundle` (its own
  `units.get` replies and the learner's lab definitions, no grader
  fields); past Academy's bundle cap (413) the course is assembled from
  `desktop.manifest` and each `desktop.unitBundle` (each Academy's whole
  bundle projection, checked for schema 1, this course, this account and
  the manifest's revision), and fetching stops (`offlineFull`) as soon as
  what was received could no longer fit one record. The bundle must name schema 1, the course, the
  signed-in account and a lease. Attempt reads (`desktop.attempt.get` of
  the signed-in account, `list`) are kept as answered online, with when
  they were kept; a draft Academy confirms (`desktop.draft.save` or
  `desktop.draft.resolve` answering `saved`) moves the kept attempt's draft
  and version forward (marking earlier step verdicts stale, as Academy
  does for a changed draft), never its lease, so confirmed work opens
  offline without another read. Media and walkthrough videos are never kept; the
  Downloads page says they, and live labs, need Academy.
- **Offline answers only when Academy is unreachable.** A learner call
  that fails with `network` or `timeout` (the access-token renewal
  included) is answered from what is kept: `units.get` and `labs.get` of a
  readable download (current or outdated, lease not ended, clock not
  behind the download time), the attempt list, and a kept attempt as
  Academy returned it only while its own lease holds (clock not behind
  when it was kept) and its course is not withdrawn; otherwise the attempt
  is answered locked, with the learner's own work and no authored
  definition. These decisions read the kept records themselves (status,
  lease, download time), not the index, so a verdict applies as soon as it
  is written; index rebuilds run one at a time. Nothing else is answered
  offline, mutations never. A reply
  clears the unreachable flag; the snapshot's `reachable` drives the
  offline notice. While Academy is unreachable the Downloads page asks for
  the course list every 15 s, so a learner waiting there sees Academy
  return without leaving the page. Restoring a saved session needs no
  network, so an offline launch opens the account's work.
- **Revalidation, never replay.** When Academy becomes reachable again,
  and when a generation opens, the core asks `desktop.access` (renewing
  each lease or marking the course withdrawn) and `desktop.manifest` (a
  different revision marks it outdated, offered for update), and reads
  each kept attempt again (Academy's reply, or its lock for revoked access
  or a withdrawn revision, replaces what was kept; an attempt Academy
  refuses is kept locked). Every read of Academy that may change a kept
  record (a foreground attempt read, a download, a revalidation) takes an
  observation stamp when it starts, and a result is written only over
  records observed before it, so a reply asked earlier and answering later
  never undoes a newer download, lock or confirmed draft (a kept newer
  draft, with its results and conflict state, also survives any older
  attempt reply); a download answered after a newer verdict is refused
  (`notDownloadable`). A generation's stamps continue after every
  persisted one, and attempt reads whose results are kept take their stamp
  only once opening finished, so ordering never depends on the clock. Withdrawals are published
  before attempts are read again. Nothing else is sent: no progress, grading, draft or submission
  is replayed, and normal token renewal sends nothing deferred.
- **Lab work kept on this Mac.** Academy's draft hook gives a host that
  keeps lab work (`localDrafts`) the latest work Academy has not
  confirmed: shortly after each edit, after a failed save and on leaving
  the lab, through one ordered queue of device operations per learner
  generation and attempt that outlives any view of the lab, so a late read,
  an older request's completion or a closed view's pending write never
  replaces newer work, and only the most recently opened view of an
  attempt may change its copy (a closed view's late replies keep and clear
  nothing). The copy is forgotten only once the kept attempt holds that
  work, so it is never the last offline copy to go. Reading the kept copy
  joins an opening in progress. "Holds" means the attempt's commit is
  known durable: a record renamed into place whose directory sync did not
  complete is not, until a later sync does, and opening a store syncs its
  directory first. Reading the kept copy joins an opening in progress;
  a copy that cannot be read is reported, never treated as none, and left
  untouched. It returns
  when the lab opens again as "saved on this device only", is never sent
  by itself, and the learner's next save sends it on the version it was
  based on, so a copy Academy moved on from becomes Academy's conflict
  with both copies shown. It is forgotten once Academy holds the latest
  work; a clear that did not complete is asked again. Grading still
  requires a draft Academy confirmed.
- **Locking and reset.** Signing out, changing account or a new learner
  generation drops the offline generation (its store closes when no job
  holds it); another account's directory and key are never opened. Reset
  purges the learner Keychain (offline keys included) and removes the
  offline root. On quit the host closes the store (`Core::shutdown`) so its
  lock is released.

### C08 implementation: management onboarding and Runtime/DLP reads

Implemented in `crates/desktop-core` (`management`: endpoints, requests,
results, the SDK binding and paging rules; `authority/management_domain`:
per-generation bindings, admission and single observation; intents
`ConfigureManagementCredential`, `RemoveManagementCredential`,
`Management`; `TenantView.management`; `WireScope::Management`),
`packages/desktop-contracts` (validators, `observeManagement`) and
`apps/desktop/src/features` (`tenants/management-credential`,
`administration`). D033 records the SDK feature and credential decisions.

- **A separate capability.** Each tenant has a scan capability and a
  management capability with their own state, storage mode and failure.
  `ConfigureManagementCredential { context, mode, clientId, secret }`
  needs the tenant's TSG (`CapabilityMissing` otherwise) and a client ID and
  secret within limits; it stores the secret like a scan key (Keychain or
  session vault, a fresh lease, the context reissued) as a
  `CredentialClass::Management` record bound to the configured token origin,
  with the client ID beside it. One credential per tenant and class;
  removing, replacing, deleting the tenant or resetting retires only the
  affected class, so a scan key survives management changes and the
  reverse. `TenantView.management` shows state, mode, failure, client ID
  and token origin, never the secret.
- **Admission.** `Management { context, request }` requires the active
  context, a ready management credential, a client ID and TSG, and a stored
  token origin equal to the configured one; then it claims a runtime
  operation. The job reads the secret off the lock (locked/denied/missing
  settle as credential failures, `NotSent`) and builds the generation's
  binding once: an SDK client with OAuth client credentials
  (`AuthContext` over the token URL, client ID and TSG), retries off, the
  app-owned transport and an `OriginPolicy` admitting only the token, AIRS
  and DLP origins, plus the `ManagementClient` and its DLP client. The
  binding and the SDK's token cache are reused by every read of that
  generation and dropped with it; retiring a generation cancels its
  session, so in-flight reads stop and their results are reported as
  cancelled, never shown.
- **Requests.** `Verify` reads one latest profile (v1 `checktenant`).
  `Page { resource, offset, limit }` reads one page (limit 1–100; DLP
  offsets on page boundaries). `Detail { resource, id }` reads a DLP item by
  its own get (404 → absent) and any other item by a bounded walk of 40
  pages of 25 (found, absent only when the service said the inventory
  ended, otherwise incomplete), as v1. Each read has a 60 s deadline.
- **Paging rules (v1).** AIRS inventories: `pagination.total_items` when
  present decides what follows; otherwise `next_offset` (zero means none; a
  cursor that does not advance is invalid); a short page without a cursor
  is the last page; a full page without one proves nothing and the next
  page is offered (stricter than v1, which treated it as the end). Profiles and topics without their array fail closed;
  applications, API keys and the DLP catalog read an absent array as empty.
  Deployment profiles and the AIRS DLP catalog are unpaginated and sliced
  locally with their total. DLP Spring pages use `last`, then `totalPages`,
  then `totalElements`, then a short page. A page holding more items than
  asked for, or over 8 MiB serialized, is invalid.
- **Data.** Items are the SDK models serialized to JSON (unknown enum values
  and fields kept), with secret fields removed recursively before they
  leave Rust. Failures are sanitized kinds (unauthorized, denied, not found,
  rate limited, unavailable, rejected, timeout, cancelled, network,
  destination, invalid response, credential locked/denied/missing,
  configuration) with the SDK's remote outcome; no server text is shown.
- **Presentation.** Settings adds the management client form (client ID,
  secret read from its field at submission and cleared before awaiting,
  storage mode, Verify, Remove) beside the scan key. Administration lists
  the active tenant's inventories (security profiles, custom topics,
  applications, API keys, deployment profiles; DLP profiles, data
  profiles, data patterns, data filtering profiles, dictionaries) 25 rows a
  page with First/Next page, Refresh and a filter over the current page
  only. It says when an inventory continues ("of N+", unread rows not
  assumed absent), reports empty only when the service did, keeps the last
  rows marked with their read time when a refresh fails, and restarts from
  the first page with nothing carried over when the tenant or management
  credential changes. Details render every field as text. Directional
  settings and customized prompt-injection modes, local previews in v1,
  are labelled as such and never read or sent. Runtime's scan profile stays
  an explicit typed name or ID; inventories never select one.
- **Operation coverage (v1 bridge `operations.ts` → SDK `=0.1.0`).**

  | v1 read | SDK call | Options |
  |---|---|---|
  | `checktenant` | `profiles().list` | limit 1, latest |
  | profiles | `profiles().list` | offset, limit, latest |
  | topics | `topics().list` | offset, limit |
  | apps | `customer_apps().list` | offset, limit |
  | keys | `api_keys().list` | offset, limit |
  | dlp-catalog | `dlp_profiles().list` | none (sliced locally) |
  | deployments | `deployment_profiles().list` | none (sliced locally) |
  | patterns | `dlp.data_patterns().list` / `get` | page, size |
  | data-profiles | `dlp.data_profiles().list` / `get` | page, size |
  | filters | `dlp.data_filtering_profiles().list` / `get` | page, size |
  | dictionaries | `dlp.dictionaries().list` / `get` | page, size, keywords false |
  | profile/topic/app/key/deployment detail | bounded list walk | 40 × 25 |

  v1's sessions and applications overviews belong to C11; writes, one-time
  secrets and multipart DLP operations to C09/C10.
- **Not done here.** No real management endpoint is called before C25.

### C09 implementation: reviewed Runtime changes and one-time secrets

Implemented in `crates/desktop-core` (`changes`: v1's write subset,
validation, digests, preview and secret removal; `authority/change_domain`:
reviews, the confirmation hand-off, apply jobs, results and held secrets;
intents `ReviewChange`, `RequestApply`, `DiscardChange`, `RevealSecret`,
`ForgetSecret`; `ShellSnapshot.changes`; adapters `OperatorConfirmation`
and `SecretPresenter`), `apps/desktop/src-tauri` (`native_confirmation`:
the AppKit sheets and the nonce registry; `clipboard_secret`: clipboard
ownership) and `apps/desktop/src/features/administration/runtime-changes`.
D035 records the decisions.

- **Review.** `ReviewChange { context, draft }` needs the active tenant's
  ready management credential (the binding source C08 checks). Rust
  normalizes the raw draft with v1's rules and the SDK request contract;
  a draft that fails yields a failed review the operator can read, with no
  request. Otherwise a runtime operation reads the baseline: nothing for a
  create, the item by a 40×25 walk otherwise (not found or an incomplete
  walk refuses), for an application delete every matching key, and for a
  key create/version the name checks and the deployment profile's auth
  code. The review holds the normalized draft, the ready mutation, the
  baseline and v1's review digest (tenant, client, mutation, baseline) for
  15 minutes; the view shows current/expected JSON (secret fields removed,
  server-determined fields labelled), the impact and the confirmation text.
- **Apply.** `RequestApply { review, draft, confirmation }` normalizes the
  freshly captured draft and requires it to equal the reviewed one, and the
  typed confirmation to equal "CREATE" or the item name; the review then
  holds a random nonce and the core asks the host's `OperatorConfirmation`
  with Rust's stored summary. A second RequestApply while confirming is
  `Busy`. Only that future's resolution (`settle_confirmation`) continues:
  a denial returns the review to ready; acceptance consumes it (a consumed
  review is refused afterwards) and admits the apply operation. A review
  whose context retires is dropped and its sheet ended (`dismiss`).
- **Write.** The apply job re-reads the baseline and recomputes the digest;
  any difference is a conflict, not sent. It then makes exactly v1's SDK
  call (`profiles/topics().create|update|delete`, `customer_apps().update|
  delete(name, author)`, `api_keys().create|regenerate|delete(name,
  author)`), with retries off, maps 409 to conflict and keeps the SDK's
  remote outcome, takes any one-time key out of the reply into a zeroizing
  buffer, removes secret fields and reads the item back (by name for a
  create, by identifier or name otherwise; absent is the expected readback
  of a delete).
- **Secrets.** `RevealSecret { operation }` passes the held key to the
  host's `SecretPresenter`; `ForgetSecret` drops it. Keys are held per
  result for the active context only, never persisted, and dropped with the
  context. The host's sheet shows the key in a selectable label, ends after
  60 s and clears the field; Copy and Close writes the key, a random owner
  marker and the concealed/transient hints, and clears the clipboard after
  60 s only if both the marker and the change count are unchanged.
- **Native confirmation.** `NativeConfirmation` registers the nonce, then on
  the main thread builds an `NSAlert` (title and message from the request,
  the confirm button, Cancel; destructive changes are critical with Cancel
  as the Return default) and begins it as a sheet of the main window; the
  completion block resolves the registry entry once. The sheet makes the
  window modal, holding the editor. Unknown and repeated completions are
  ignored; dismissal resolves false and ends the sheet.
- **Presentation.** Administration offers v1's actions (new profile/topic,
  create key; edit/delete; rotate/add version/delete) in a workbench whose
  editor is uncontrolled: Review and Apply read it when pressed; Apply…
  also reads the typed confirmation. While confirming the editor and the
  confirmation field are read-only. Applied changes list their outcome and
  readback, and a held key offers "Show new key…" and "Clear key".
- **T23.** Reviews, confirmations, results and keys are memory-only: a
  restart shows none and nothing is replayed; offline, a review that needs
  a read fails and a confirmed write reports its unknown outcome, never
  queued; retry is an explicit new review.
- **Not done here.** No real management endpoint is called before C25; DLP
  writes are C10.

### C10 implementation: supported DLP authoring and multipart workflows

Implemented in `crates/desktop-core` (`changes`: DLP kinds, v1's
`validDataProfile`, dictionary bodies; `authority/change_domain`: DLP
baselines and readback by get, staged keyword files; intent
`ChooseDictionaryFile`; `ChangesView.staged`; adapter `text_files`),
`management` (`dlp_item`, the DLP writes and the dictionary multipart
upload), `apps/desktop/src-tauri` (`NativeTextPicker`, the fixture's staged
file) and the Administration DLP actions and dictionary workbench. D036
records the decisions.

- **Capability table (v1 → SDK `=0.1.0`).**

  | Kind | Create | Replace | Delete |
  |---|---|---|---|
  | Data patterns | `data_patterns().create` | `replace(id)` | `delete(id)` |
  | Data profiles | `data_profiles().create` | `replace(id)` | — |
  | Data filtering profiles | — | `data_filtering_profiles().replace(id)` | — |
  | Dictionaries | `dictionaries().create(DictionaryUpload)` | `replace(id, DictionaryUpload)` (204 → no body) | `delete(id)` |
  | Deployment profiles, AIRS DLP catalog | read only | read only | read only |

- **Review and apply.** Exactly as C09: a DLP item's baseline is its own get
  (404 refuses the review), the confirmation is "CREATE" or the item's
  name, and the apply re-reads it and refuses on any change. Readback reads
  the item by the reply's `id` (a create) or the target; a dictionary
  replace that replies 204 is read back the same way; absent after a delete.
- **Dictionaries.** The workbench holds the metadata JSON and either typed
  keywords or the chosen file; Review and Apply build `{"metadata": …,
  "file": …}` or `{…, "fileRef": token}` from the fields when pressed. Rust
  resolves `fileRef` to the staged text of the active context, validates
  the body and uploads the SDK's multipart: part `json` (`metadata.json`,
  application/json) and part `file` (the `original_file_name`, text/plain),
  under a checked `scm-sdk-` boundary within the 8 MiB request limit.
- **Keyword files.** `ChooseDictionaryFile` opens the native sheet (one at a
  time); the host reads at most 256 KiB of a regular file; the core accepts
  UTF-8 text (byte-order mark dropped) that is non-empty and at most 64 Ki
  UTF-16 units, reports too large / not text / empty / unreadable, and
  stages it with a random `sf-` token for the active context only (dropped
  when the context retires). The snapshot shows its name, size and line
  count, never content or a path; the source file is only read.
- **Not done here.** No real DLP endpoint is called before C25.

### C11 implementation: Sessions investigations and metadata-only reports

Implemented in `crates/desktop-core` (`sessions`: classification, v1's
verdict and detector rules, the report model and CSV, the investigation
cache and its sealed persistence, the report file; `management`: the
`Sessions` page resource, `Session`/`Transaction`/`Content`/`Search`
requests and the bounded walks; `authority/sessions_domain`: reports,
cache clear/export and the save sheet; intents `OpenReports`, `SaveReport`,
`RemoveReport`, `UndoReport`, `ExportReports`, `ClearInvestigation`,
`ExportInvestigation`; `ShellSnapshot.sessions`; adapters `saver` and
`sessions_root`), `apps/desktop/src-tauri` (`NativeSaver`, the fixture
saver) and the Sessions & Reports destination. D037 records the decisions.

- **Reads (v1 → SDK `=0.1.0` raw dashboard).**

  | Read | SDK | Parameters |
  |---|---|---|
  | Inventory page | `sessions_overview_raw` | `time_interval=1`, `time_unit=day`, `limit=25`, `offset` |
  | Session | `session_raw` | session, app ID, app name, `offset=0`, `limit=25`, 30 days |
  | Transaction | `session_transaction_raw` | session, app, name, scan, sub-request |
  | Content (reveal only) | `scan_content_raw` | scan, sub-request |
  | Search walk | `sessions_overview_raw` | `limit=100`; 30 days × 40 pages, or 1 day × 1,000 sessions |

- **Content.** Every read except Content has secret fields redacted and
  `prompt`, `response`, `scan_contents` and masked data removed. The
  presentation holds revealed content in component state keyed by scan, so
  Hide, leaving Sessions or a new context drops it.
- **Cache.** Per credential generation (`GenerationBinding.investigation`):
  sessions, transactions and revealed contents, 50 each, newest first. A
  Keychain credential's cache loads its sealed snapshot once, merges older
  entries behind newer ones and is re-sealed after each read (one writer at
  a time, the cache as it is then; empty removes the file). The key record is
  `cr-` + the first 16 hex of SHA-256("investigation-key:" + record), the
  file `sessions/investigations/<digest>.sealed`; `drain_cleanup` removes
  both with the credential, reset removes the sessions root.
- **Reports.** `OpenReports` loads the active tenant's file off the lock
  (an unreadable file is reported and kept). `SaveReport` edits an existing
  report (kind, category, reason; metadata and time kept) or builds a new one
  with `report_of` from the cached session's matching action row; refusals
  publish their `ReportProblem`. Remove keeps one undo; Undo never restores
  over a newer report. Each change bumps a generation and only the newest is
  written (0600, atomic); a failed write is shown as unsaved. CSV is v1's 16
  columns with the formula guard and CRLF, named
  `fpfn-reports-<slug>-<YYYYMMDD-HHMM>.csv`.
- **Budgets.** The large management collections workload is measured by
  `tools/perf/measure_c11.py` against the budgets registered before C11
  (`evidence/C11/workload-budget-registration.json`).
- **Not done here.** No real dashboard endpoint is called before C25.

### C12 implementation: Case Library and Runtime Home/Inbox observations

Implemented in `crates/desktop-core` (`library`: the case model, drafts,
limits, export/import, catalogs and the sealed file; `observation`: the
chart contract, Runtime summary, key-expiry and activity items and their
order; `authority/library_domain`: per-tenant libraries, imports, catalog
reads and copies; `authority/observation_domain`: the overview record,
freshness, attention faces and holds; `ManagementRequest::Overview`;
intents `OpenLibrary`, `SaveCase`, `DeleteCase`, `ImportCases`,
`ImportExamples`, `CopyCase`, `ExportCases`, `HoldInsight`,
`RestoreInsight`; `ShellSnapshot.library` and `.overview`; adapters
`case_files`, `library_root`; `CoreConfig.case_catalog`),
`apps/desktop/src-tauri` (`NativeCasePicker`, the fixture case files and
catalog URL) and the Case Library destination and Runtime Overview & Inbox.
D038 records the decisions.

- **Case compatibility and execution.**

  | Product | Fields | Execution |
  |---|---|---|
  | Runtime Security | name, profile, application, prompt, response, goal | Fills the scan form (Load / starters); a scan is one explicit press |
  | AI Gateway, AI Red Teaming, Model Security, Skill Scanning | name, specification, response, goal, payload object | None: inspect and transfer only |
  | Unknown product | refused by the wire contract and by import/catalog parsing | — |

- **Storage.** `library/<digest>.sealed`, key record `cr-` + 16 hex of
  SHA-256("case-library-key:" + tenant); loads off the lock; an unopenable
  file becomes `.unreadable`; writes by (tenant, generation) under the
  local-data writer, which `ForgetLibrary` (tenant deletion) and reset take.
- **Overview sources.** `sessions_chart_raw` (1 day) validated like v1
  (bucket size ≥ 1 s, ≤ 2,048 buckets, parseable times, integer totals) and
  the API key list (first page of 100, secrets redacted). Each source keeps
  its data or failure; gaps name what is loading, unavailable, partial or
  inconsistent. Faces: Items, First launch, All clear (fresh, both sources
  complete, no gap, nothing open), Gap.
- **Not done here.** No real endpoint is called before C25; Gateway data is
  added to the same observation presentation in C18.

### C13 implementation: Red Team and Model/skill observation

Implemented in `crates/desktop-core` (`red_team`: v1's statistics, list,
attack, skill-statistics rules, completeness and skill pseudonyms;
`management`: `ManagementEndpoints` product planes, the Red Team, Model
Security and AgentGuard SDK clients in `ManagementBinding`,
`ManagementRequest::{RedTeam, SupplyChain, Attacks}` and their results)
and the Red Team & Models destination (`features/red-team`). D039 records
the decisions.

- **Reads.**

  | Request | Service reads | Result |
  |---|---|---|
  | `RedTeam { window }` (7, 15, 30) | Red Teaming data plane `GET /v1/dashboard/scan-statistics?date_range=LAST_N_DAYS`; on 404, `GET /v1/scan?skip=0&limit=1` to confirm no scans; `GET /v1/scan?skip=0&limit=100` | statistics (totals, status, risk and target-type bars) and the first 100 scans (id, status, job type, target id and type, time), each with its own failure |
  | `SupplyChain` | Model Security `GET /v1/scans?skip=0&limit=100&isBackgroundRefresh=false`; AgentGuard `GET /v1/stats/scans?time_period=30_DAYS` and `GET /v1/scans?skip=0&limit=100&isBackgroundRefresh=false` (with `x-tsg-id`) | model scans (id, outcome, time), skill statistics (unique skills, vulnerabilities), skill scans (pseudonym, status, outcome, vulnerability count, time) |
  | `Attacks { scanId, page, received }` (UUID, 0-9, rows received before it, at most 100·page) | Red Teaming `GET /v1/report/static/{scan}/list-attacks?skip=100·page&limit=100` | numbered rows (succeeded/defended/not rated, severity, type, sub-category, turns), total, capped, completeness |

- **Validity and coverage.** A reply reporting a failure (`success: false`,
  an `error` or `errors` field) is invalid. Statistics must also carry one
  of their known fields; a breakdown has at most 64 entries and labels
  outside v1's list for it are OTHER. A missing total is zero only when
  every reported count (totals, every breakdown entry, every rated target;
  AgentGuard: both counts and no top vulnerability) is zero; every reported
  rated-target group is validated first (an array of at most 64 objects,
  each with a whole, non-negative count; an absent or null group is
  unreported, a narrow tolerance v1's parser did not have). A list needs its pagination object
  (the total may be omitted); a total below its page's rows, or a present
  non-boolean `count_capped` (null included), is invalid, and such a page
  never proves "no scans"; every
  list carries `capped` (the total is then a lower bound, shown as "≥ N")
  and is complete only when uncapped and its total is no larger than the
  rows actually received (attack pages: `received` plus the page), or,
  without a total, the page is short.

- **Presentation.** Two tabs, each read when first shown with nothing held,
  on Refresh and (Red Teaming) a window change; tiles, ranked lists, source
  coverage (complete, partial sample with lower bounds, unavailable), the
  scan table with a local status filter, attack results read page by page
  on opening, each page carrying the rows received so far, until the total
  is covered, a page comes back short, a read fails or the ten-page cap is
  reached (v1 read two pages and offered Load more); "no attacks" only for
  a complete read, and a disclosure of
  the workflows that are not available. The view is keyed by context; a failed refresh keeps the last
  read with its time; a superseded reply is dropped.
- **Not done here.** No real endpoint is called before C25; no scan,
  target, adapter, channel, prompt set, ASR verification or Model Security
  workflow exists; Home tiles and observation history for these products
  belong to C16-C18.

### C14 implementation: API Client projects and documents

Implemented in `crates/desktop-core` (`workbench::{project, document,
environments, profile, schema, curl, insomnia, git, scm, adoption}` and
`authority/workbench_domain`: `Intent::Workbench { request }`, results
observed once through `ViewScope::Workbench`, `ShellSnapshot.workbench`,
adapters `folders`, `workbench_files`, `workbench_root`, `CoreConfig.git`
and `.adoption_source`), `apps/desktop/src-tauri` (native folder and export
sheets, File > Save, the app's Quit item and the close guard) and the API
Client destination (`features/workbench`). D040 records the decisions.

- **Ownership.** React owns the editor text; Rust owns each document's
  baseline, revision, faithfulness check, conflicts, journal and sealed
  drafts. An opened document's fields come from the metadata its baseline
  hashes, never from the (abbreviated) inventory. Every request runs in admission order on the workbench lane
  (reset takes it too) with file work off the authority lock.

  | Request | Effect |
  |---|---|
  | `save { id, revision, fields, edit }` | stale revision refused; the submitted snapshot sealed as the draft before any project write (a snapshot equal to the saved version is `unchanged` and removes any older draft, stored before the reply); `check` (Fix to save with field/row); inventory complete, identity unmoved and not duplicated; a body that reaches a file something else owns refused (`sharedBody`): another request file's body (duplicates and invalid ones included), a schema a request associates, or a project file (any request file, `collection.toml`, an environment, a standalone schema; a `.json`/YAML file a request names as its body is that body), compared by the file reached (case-folded name, or the same device and inode) rather than its spelling; files equal the baseline (else conflict); journal, body, metadata, journal cleared; new revision |
  | `protect { id, revision, fields, edit }` | the snapshot (valid or not) sealed as the document's draft |
  | `editing { id, edit }` / close | the close guard's unprotected mark keeps the newest edit generation reported; a Protect or Save clears it only when its snapshot covers that generation. Closing a tab or the collection with a mark is refused (`unprotected`) unless the operator discards; the editor protects its latest text first and asks when that fails |
  | `resolve { choice }` | both versions archived; Use disk reopens; Keep mine rebases |
  | `preview { id, fields }` | v1's variable layers, secrets masked, unresolved and missing listed |
  | imports, environments, secrets, schemas, Git, adoption, built-in | see D040 |

- **Execution (C15).** See below.

### API Client execution (C15)

Implemented in `crates/desktop-core` (`workbench::{render, admission,
execute, history, cookies}` and `authority/workbench_domain/send`), the
host's save-response sheet and the destination's Send, review, response
and History views. D041 records the decisions; ADR 0004 the policy.

- **Pipeline.** `send { id, revision, fields, context }` takes its turn on
  the workbench lane: the document's revision is checked, the snapshot is
  rendered (secrets stay slots naming their references), the tenant's TSG
  is filled in and `admission::admit` classifies the request and its
  destinations for the build's network mode, before any secret is read.
  Ordinary and known-read requests then leave the lane: secrets are read,
  `execute::prepare` builds the request and `execute::send` streams the
  response into sealed history; the single result is the run.
- **Reviews.** A known tenant change returns `reviewRequired` with a masked
  view and a digest of everything bound; `apply { review, … }` re-plans the
  same snapshot after the native confirmation, compares the digest,
  consumes the review and sends once.
- **Registered origins.** `Destinations.registered` holds every configured
  management, token and workbench API origin, the four AIRS scan regions
  and each tenant's scan destination (by host and port), built with or
  without an active tenant. A request there is classified whatever its
  credential (manual header, collection secret or none): known mutations
  are reviewed, unknown operations denied; URL user-info and Host or
  forwarding headers, the editor's or one its authentication generates
  (an API-key header named Host), are refused there and for every managed
  request; there a method override or a second credential (Authorization,
  Proxy-Authorization, Cookie or x-pan-token, the request's own
  authentication whatever header it generates, an API key in the query, or
  the collection jar's Cookie for the URL, read at planning) is refused
  too, while one manual Authorization or the session cookie alone still
  goes; a generic redirect never enters one.
- **Context binding.** A context change (switch, credential replacement,
  reset) retires the pending review for good; Apply checks the active
  context; a managed or reviewed request re-checks its context and the
  tenant's credential records before its secrets are read, before a token
  is minted and before the request is sent (a redirect's later hops follow
  a request already sent), ending `retired` with nothing sent.
- **Credential binding.** A review's digest also binds the authentication
  in full (Basic username, Basic/Bearer/API-key secret references), each
  workbench secret's generation this session (Set secret starts a new one
  before the Keychain record changes) and the hash of the Cookie header the
  collection's jar gives the URL. Apply, after the native confirmation,
  must reproduce it; a reviewed request also re-checks these before it is
  sent and carries the admitted Cookie header on its first hop instead of
  reading the jar again.
- **Reset fence.** Each send keeps the reset epoch it was admitted in,
  sampled before its first await; reset advances the epoch when admitted.
  A send retired during planning loads no jar, publishes no review and
  registers no run. A send's private-storage steps (history and
  cookie loads and writes, the response body file, pruning) each hold a
  shared fence and run only in their epoch. Reset holds the fence
  exclusively through its purge, so no retired send recreates the folder,
  its key, history, bodies or cookies, or republishes its record (it ends
  `noCollection`).
- **Redaction.** Response headers and redirect targets are redacted of the
  secrets used and of the credentials derived from them on the wire (the
  minted bearer, Basic encodings).
- **Views.** `WorkbenchView.running` (four at most, with bytes moved),
  `.review` and `.network`; results `ran`, `reviewRequired`,
  `sendRefused`, `history`, `responseSaved`, `saveResponseFailed`.

### C16 implementation: Gateway workspace and configuration reads

Implemented in `crates/desktop-core` (`gateway`: the advertised reads, v1's
projections and completeness rules; `management`: the Gateway planes in
`ManagementEndpoints`, the SDK's `AIGatewayClient` in `ManagementBinding`,
`ManagementRequest::Gateway` and its result; `authority/gateway_domain`:
the workspace listing, the native choice, restore and admission;
`context::WorkspaceRef`) and the AI Gateway destination
(`features/gateway`). D042 records the decisions.

- **Reads.**

  | Request | Service reads | Result |
  |---|---|---|
  | `Inventory { workspaces }` | data `GET /workspaces` | listed workspaces (id, name, slug, IAM scope name); recorded for the context |
  | `Inventory { providers \| routes \| guardrails \| serviceKeys \| userKeys }` (scoped) | data `GET /providers`, `/configs`, `/guardrails`, `/api-keys/service`, `/api-keys/user` with `workspace_id` | projected rows, count, total, completeness |
  | `Inventory { integrations }` | admin `GET /integrations` | projected rows (tenant-wide) |
  | `Detail { resource, id }` | the matching `GET …/{id}`; for an item naming no workspace, also its inventory | the projected item, or absent/incomplete |
  | `IntegrationModels { id }` | admin `GET /integrations/{id}/models` | model slugs, names, enablement, all-models flag |
  | `Prerequisites` (scoped) | routes, providers, service keys, integrations and IAM `GET /scopes/{scope_name}` | each source or its failure, the scope binding, each plane's access, inference-scoped key count |

- **Choice and admission.** The workspaces listed under the current context
  are the only ones that can be chosen; a choice issues a `WorkspaceRef`,
  is saved per tenant and retires the reads of the one before (their
  operations run under the reference). Scoped reads are admitted only
  with the current reference and read the workspace it names; a result
  settled under an earlier choice is withdrawn when observed. A context
  change clears the listing and the choice; a later listing that lists the
  remembered workspace restores it, and a complete one without it says it
  is gone. Runtime reviews are untouched.
- **Presentation.** The workspace is a native radio group; inventories are
  tabs read when first shown, with coverage that never calls a partial
  inventory empty; a detail's heading takes focus and Close returns it;
  credential-bearing configuration says so without values; the
  prerequisites tab states each source, the scope binding and each plane,
  and that no inference credential is configured. Keyed by context; a
  reply for an earlier choice is dropped.
- **Not done here.** No real endpoint is called before C25; nothing is
  created, changed, provisioned or sent for inference; limits policies,
  telemetry, cost and MCP are later chunks.

## 3. Native Harness, identity and protected storage

Retain engine 9331a5839bc6a763d94dee7584746f58bbbe1181 and protocol airs-desktop/1
as the initial source candidate. Spawn the sealed Rust binary directly with
`desktop-server`. Bootstrap credentials travel only over bounded stdin; no
inherited credential environment, command-line secret, npm launcher or PATH fallback.
Replace host Academy/Harness/MCP workers with Rust before removing Node resources.
Rebuild/reseal and sign the exact new native inventory; prove operation without
Node/managed CLI and revise engine instructions/contracts only if needed.

The lifecycle interface is start(scoped bootstrap) → supervised run, send(typed
protocol input), observe(bounded events) and stop(reason). The native core owns
handles and grants; React never spawns processes or supplies executable paths.
Stopping retires run-specific approvals/grants (the narrowly scoped MCP-only
continuity rule below can reissue project access to a new run), closes pipes, waits a bounded grace period and
forces/reaps the owned process tree if necessary. Sleep, app close, tenant/credential/
project changes or unexpected EOF cannot leave an authorized orphan. MCP approval
binds the actual running call item, server revision/grant, arguments and conversation;
matching tool names/arguments alone is insufficient. Retain eight-server/64KiB
bootstrap limits unless a separately reviewed protocol change qualifies them.

Initial stop budget is 2s graceful plus 2s forced/reap; failure remains a cleanup
error, not success. Only the spawned sidecar/process group is owned; never act on
a renderer PID or a recycled process identifier. C19 qualifies those limits with
the actual engine and permission enforcement.

Preserve read-only selected-project defaults, approved single-process restrictions,
no pipelines/descendants/build/test runners, no engine Apply and Gateway-only
inference/MCP routing. Production never passes the engine's hidden
--allow-http-loopback flag. WorkspaceWrite requires explicit operator grant for
the selected project/run and fresh confirmation after a Gateway-credential
refresh restart. The separately specified MCP-only rotation retains that current
conversation's already-confirmed project access. History alone grants neither. Native tools use the principal-restricted authority handle.
History is encrypted user work, separate from redacted diagnostics. Imported
history is inspect-only until protocol/context compatibility is separately proven;
never restore pending approvals or old authentication.

### Harness refresh and in-session continuity

The pinned engine accepts a Gateway credential only in one-shot bootstrap api_key;
refresh cannot mutate it in place. Engine ContextBinding credential_id and
credential_epoch identify the stable native grant and its authority epoch, never
the immutable vault record ID or token-storage revision. Harness ContextRef uses
that grant epoch too. Refresh and re-sign-in to the same verified subject/issuer/
audience/workspace/origin under a still-live grant preserve these values and all
other ContextBinding fields, so the engine's exact replay.context == context
check accepts compatible history. New vault records/token leases still prevent
obsolete async completions from overwriting current material without changing
the conversation identity. Explicit credential replacement, identity/origin/
configuration change, sign-out or grant revocation retires/bump the grant epoch,
invalidates ContextRef and requires a new conversation; re-sign-in after explicit
retirement cannot revive it. ContextBinding equality includes tenant_id,
workspace_id, credential_id, credential_epoch, gateway_endpoint, project_path,
local_access, gateway_config, model and the exact MCP binding inventory. Any
change to these fields starts a new conversation; preserve the former protected
history for inspection, not a mismatched restore. Before an explicit turn, compare wall-clock
expiry with a safety margin min(120s, token lifetime/2), including after sleep.
Keep the old run attached while refreshing. If refresh succeeds for the same
verified subject/issuer/audience/workspace and current native grant, stop/reap it,
bootstrap an identical scoped run with the new credential, then use
`desktop/history/restore` for compatible in-session history before sending that
explicit turn. Re-ask WorkspaceWrite permission before stopping for restart;
decline cancels the send, keeps the draft and does not restart or restore the
conversation. An old run may remain attached for inspection, but it cannot send
an expired credential. The operator may explicitly start a new read-only
conversation; that is a changed binding and does not resume/replay this one.
Fresh tokens do
not silently revive retired context/approvals. Refresh failure retains visible
history/draft, sets SignInNeeded and never sends with an expired token.

If expiry/unauthorized failure interrupts a turn, preserve actual outcomes. Restore
only the local pre-turn history/prompt checkpoint when no tool call, approval or
native handoff effect has started; otherwise retain effect entries and their
known/unknown outcomes and offer explicit recovery. This is local presentation
rollback, never remote/file rollback or automatic inference/tool replay. Judge
expiry from wall clock and actual event-arrival timestamps, not when a queued UI
event drains. Identity replacement retires the old run and creates a new
conversation; it cannot restore the previous identity's authority. C20/W43 tests
exact ContextBinding equality and successful refresh/re-sign-in/history restore,
rejection after credential replacement/epoch bump, failed refresh, expiry across sleep,
delayed events, mid-turn effects and repeated workspace-write confirmation.
C20/C21 assert decline retains the draft/cancels Send without restart, and no
restore is attempted after local_access/model/configuration/MCP binding changes;
the new conversation never replays a prior turn.
Compatible in-session restore is distinct from inspect-only imported history.

### MCP token rotation and native conversation scope

MCP access tokens also enter the engine only through one-shot bootstrap. Before
each explicit turn, refresh the recorded scope's eligible MCP tokens in Rust,
then check the Gateway credential. If only MCP material changed, stop/reap the
old engine, bootstrap the identical ContextBinding with current tokens, restore
compatible history, and then send that explicit turn once. Preserve working v1:
MCP-only rotation neither refreshes SSO unnecessarily nor re-asks WorkspaceWrite.
Native continuity reissues the conversation's current confirmed project access to
the new run; old process grants, tool-call approvals and callback nonces retire.
If Gateway refresh is also needed, combine the restart and apply its write-access
confirmation/decline rule once. Any changed authority cancels this continuity.

The native protected conversation records every MCP namespace/server ID/canonical
URL/catalog revision/grant ID in fixed order, plus its project-access grant. The
engine McpServerBinding records only name/serverId/url; it excludes grant IDs and
catalog revisions, so engine equality is necessary but insufficient. Rust
revalidates the recorded native scope before refresh, bootstrap, history restore,
turn admission and tool approval. Re-sign-in creates another MCP grant even for
the same account/URL; that, explicit sign-out, removal or revision change retires
the conversation and requires a new one. Today's enabled-for-new-conversations
set does not rewrite an existing scope. Missing/loading/unavailable catalog
blocks remote resume/Send until checked; it never grants unknown authority.

Token expiry or failed refresh alone does not retire a still-recorded MCP scope.
Keep the conversation inspectable and, while Gateway inference remains valid,
sendable without that server's tools. A usable held token may continue until its
wall-clock expiry; after expiry use the fixed noncredential placeholder
`airs-desktop-mcp-sign-in-expired` for that recorded server in bootstrap, retaining
engine binding equality and non-required startup semantics. Gateway rejects it;
show SignInExpired/tools unavailable. Native approval independently denies calls
for the expired server even if a server advertises anonymous tools. Never borrow
a different grant or replay a failed tool call. Re-sign-in offers a new
conversation and preserves the draft/history. C22 tests equal-binding MCP-only
rotation/no replay/no extra write prompt, combined Gateway refresh confirmation,
same-URL different-grant/revision rejection before engine restore, expired-server
startup with continuing inference, failed refresh and approval denial.

Native identity mechanics use openidconnect 4.0.1 for OIDC discovery/code/ID-token
validation and oauth2 5.0.0 for device/refresh operations, both with default-features
false and the shared custom async HTTP adapter. C01/C04 qualify and lock
the actual compatible dependency graph. Use a bounded no-redirect async HTTP
adapter, configured issuer and explicit token/JWKS destinations. Discovery issuer
must match; metadata does not grant arbitrary new destinations. Browser PKCE S256
uses random state/nonce, loopback-only ephemeral callback and bounded request lifetime.
Device polling honors pending/slow_down/expiry/cancel. Refresh is serialized per
grant, rotating storage atomically; revocation/sign-out retires only that identity.
See [OIDC crate documentation](https://docs.rs/openidconnect/4.0.1/openidconnect/).

Signature/JWKS verification is a stricter v2 addition to v1 host behavior.
Use the OIDC verifier for ID tokens; use jsonwebtoken 11.1.0's verified decode
for required JWT Gateway access-token claims. Never treat an unverified decode
as authentication. Reject none/symmetric confusion; accept only configured,
provider-supported asymmetric algorithms/key types. Validate issuer, audience,
authorized party/client where required, nonce for ID tokens and exp/nbf/iat with
bounded skew. Unknown kid allows one bounded JWKS refresh from the configured
endpoint; failure rejects, not signature bypass. Opaque Academy OAuth tokens are
used only with the hosted Academy verifier; do not invent local JWT claims for
them. Opaque Gateway tokens require a separately reviewed introspection contract
or are unsupported in the first SSO path. See
[JWT validation documentation](https://docs.rs/jsonwebtoken/11.1.0/jsonwebtoken/).

Proposed independent identities:

| Item | Value |
|---|---|
| Display name | Prisma AIRS Desktop v2 |
| Bundle identifier / Keychain service | io.cdot.prisma-airs-desktop.v2 |
| Data root | OS Application Support / io.cdot.prisma-airs-desktop.v2 |
| Logs/cache | Separate OS v2 directories; logs contain redacted diagnostics |
| Credential accounts | Versioned opaque IDs scoped by tenant/capability/grant |
| Academy work | Separate issuer + learner-subject + organization + schema scope |
| Harness work | Tenant/workspace/conversation scope; grant IDs govern resumption |

The C02 macOS credential adapter uses security-framework 3.7.0 classic login
Keychain generic-password items (service=v2 identity, account=opaque versioned
record ID), retaining the v1 storage class without its namespace. Use macOS
SecKeychain generic-password calls, local/non-synchronizing items, no shared
access group and default application ACL; do not choose the data-protection
Keychain or claim iOS WhenUnlockedThisDeviceOnly attributes apply to classic
macOS storage. Availability follows the login Keychain's lock state and OS ACL
prompts. A denied/cancelled prompt is LockedStore/DeniedStore, not permission to
weaken the ACL or persist plaintext. Native IO runs off the transition lock.

Unsigned/ad-hoc fixture builds use a dedicated test Keychain and fixture service;
identity/ACL failures are expected and explicitly tested. Production uses signed
Developer ID code with stable Team ID/bundle designated requirement; C26 tests
same-identity upgrade access and denial for unrelated code, with recorded OS
prompts. C02 tests locked/denied/missing items using synthetic secrets and never
loosens real user ACLs. Signing qualification is separate from a fixture pass.

No mutable v1 namespace is shared. Protected work uses authenticated encryption
with a random per-scope key in v2 Keychain; XChaCha20Poly1305 0.11.0 is the proposed
cipher implementation. Fresh random nonce per write; associated data binds record
ID, account scope and schema. Directories/files use owner-only modes, atomic
replacement and a process-wide store/reset lease. Locked/denied storage never
falls back to plaintext. Session-only credentials remain in memory and are
cleared when the scope/app ends. See
[RustCrypto implementation](https://docs.rs/chacha20poly1305/0.11.0/chacha20poly1305/).

Imports are explicit dry-run selections from a versioned source snapshot. Read
only selected work/nonsecret metadata; never follow credential-file references.
Reencrypt selected work into a new v2 generation, validate it, then atomically
commit the v2 manifest with a restartable import receipt. Preserve the previous
v2 generation for rollback. Never mutate source bytes or copy credentials,
sessions or approvals. Require fresh sign-in/account verification before importing
account-bound learner work; caches/downloads are regenerated. An interrupted import
cannot expose a partially committed generation; unsupported downgrade fails
before touching data. C23 validates every failure/commit point.

## 4. Managed workbench execution

Keep RequestDocument semantics and immediate React editor truth separate from
execution permission. Send captures fresh complete input; native admission resolves
variables and references, validates method/URL/headers/body and constructs one
immutable effective request before attaching managed secrets.

The credential class is explicit: tenant-managed scan/management/Gateway grants,
workbench-owned secrets, or anonymous. Never infer the class from redactions.
Replace v1's trust in the editable `prisma-airs-scm` collection ID: no imported
ID, request file, tenant reference or metadata flag grants access to tenant
secrets. Native authority registers explicit tenant/workspace capabilities for
the selected operation. Changing a collection ID cannot create that registration.
Classify every requested tenant reference as managed before resolving it,
independent of collection provenance. In particular, admit a managed OAuth token
destination before obtaining client secrets; edited token_url variables cannot
send an existing grant to a new origin. Legitimate endpoint changes follow the
explicit profile/capability destination-review flow, invalidating old authority.
Known AIRS operations use a versioned method/path/destination catalogue:

- Known reads: admit only with their actual capability and approved destination.
- Known mutations: require a Configuration review of the exact effective request.
- Unclassified managed requests, including unknown reads: deny without dispatch;
  do not offer a renderer 'override' or blindly review an unknown operation.
- Classification also checks registered Prisma origin/method/path independent of
  credential class. Manually entered workbench credentials cannot bypass review
  for known configuration mutations; unknown operations at registered managed
  origins are denied until catalogued. Redaction is not classification.
- Generic HTTP to other destinations with workbench-owned credentials/anonymous
  access retains operator-directed behavior and cannot borrow tenant credentials.
  Arbitrary third-party proxy semantics cannot be inferred universally; this rule
  covers native-managed destinations and explicitly registered Prisma origins.

A workbench review includes expanded URL/query, method, material headers, body/
file hashes, cookie snapshot, resolved variable revision, credential revision,
destination and resource/action classification. Secret values are never included
in the review/model/log projection. Dispatch rechecks all bindings; changed
input/files/environment/credentials/cookies/context requires a new review.
Reject ambiguous path normalization, encoded separators/traversal, duplicate auth
headers, URL user-info and attacker-controlled managed proxy/Host overrides.

Disable automatic redirects for managed requests in the first implementation.
Show the 3xx result; following it is a new explicitly prepared/admitted request,
including same-origin redirects. Workbench-owned redirects retain bounded generic
HTTP behavior with explicit credential/cookie origin rules. This is stricter than
v1's redactions-based cross-origin redirect guard and must be disclosed/tested.

Remove v1's automatic post-401 resend for all workbench requests. Token refresh
may prepare a new explicit Send; it never repeats the customer operation. Lost
responses/timeouts remain uncertain after dispatch. No auto-replay on reconnect,
credential refresh, history replay, imported requests or engine suggestion.
The 193 catalogue entries (81 mutation-method specifications) are metadata parity,
not a claim of 193 dedicated workflows or live qualification.

## Validation and remaining gates

desktop-host owns the OS process-spawn/process-group adapter and browser opener;
desktop-core Harness owns lifecycle and conversation policy through those seams.
C19 sidecar_supervision host tests cover actual spawn/cleanup, and C04/C20/C22
browser_callback host tests cover approved OS-browser opening/callback bounds.

Host package tests exercise production desktop-host admission and OS adapter
interfaces: C01 ipc_admission/permission_manifest, C02 ipc_admission/keychain_access,
C09 native_confirmation/clipboard_secret. They cover wrong window/origin,
unknown/oversized DTOs, production permission/CSP and fixture-command exclusion,
private callback forgery/repetition, sheet context retirement and clipboard
ownership/timers. Actor tests in desktop-core cover domain decisions; host tests
cover native wiring/adapters. desktop-host test-fixtures enables tauri/test for
its documented mock runtime admission tests; the real WebView/native-window
paths are additionally exercised by native-prepare and operator/CUA acceptance.
Actual main-window sheet/focus/OS behavior also
requires native acceptance; neither layer substitutes for the other.

Tests cross these production interfaces with native SDK/scripted transport,
Keychain/session/test-store, packaged/literal-process and real/manual-clock
adapters. Keep literal external request/protocol expectations independent of
implementation builders. Native Tauri UI, live scope and installed-package proof
remain separate evidence. A favorable design score cannot replace them.

C01/C02 qualify actual dependencies and native tooling before source acceptance.
This host is Mac17,3 / Apple M5 / 24GiB / macOS27.0.1 (26A434). Its execution
PATH initially omitted Rust; absolute-path inspection found Rust/Cargo1.90.0
and the aarch64-apple-darwin target under ~/.cargo/bin. Node26.11.0 resolves by
default; Node24.21.0/Corepack is also present. pnpm10.6.5 remains to be resolved
through the build-tool preflight. xcodebuild selects CommandLineTools. No tools
have been installed and no build is claimed.

The design handoff includes the command/evidence manifest in qualification.md/
checks.json, the workflow/live expansion in workflows.md and the source
disposition in ../planning/distribution.md. C01 resolves/qualifies actual locks
and shared extraction; later family operation/options map before their consuming
chunk. The independent selected-design verdict/readiness is tracked in planning/status.md. D024 selects
Apache-2.0 for newly authored material; D025 records the owner's report-rendering
confirmation. Browser automation remained unable to inspect local HTML. These
records do not constitute application/native qualification;
this document does not unlock C01 by itself.
