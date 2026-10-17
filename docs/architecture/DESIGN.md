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
