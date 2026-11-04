# Architecture candidate research

Date: 2026-10-16. Scope: read-only preparation for G01. No concrete interfaces
were selected at the start of research, no application exists, and G01 is not passed. The owner
expected continuing progress; this research proceeds within the existing
authorization for planning/source inspection without inventing further product
questions. The owner subsequently selected all four candidates in the recommended
order through the architecture selection question. Concrete interfaces now
belong to the selected-design work. D024 now selects Apache-2.0 for newly authored v2 material.

The temporary visual report is outside the repository:
`/var/folders/gb/80rbbhr936z3k_4n4m3fr0j00000gn/T/architecture-review-20261016-163533.html`.
Its SHA-256 is
`237c99460f5dfaabcb746aa5018824088dff67bdd062b7e8eb0285ee040b081b`.
It was opened through macOS `open`. Browser automation rejected local-file
inspection because its URL policy allows only HTTP/HTTPS. No workaround was
attempted. The owner subsequently confirmed that the four cards and diagrams render (D025);
this is owner visual confirmation, not an automated screenshot. Essential CSS and
candidate SVG/HTML diagrams are inline; CDN failure cannot remove that content.
The Mermaid relationship diagram is supplemental.

## 1. Rust operation authority — Strong

Observed: v1 `src/workspace.rs:333`, `src/gateway/tenant.rs:4`,
`src/request.rs:557`; v1 ADRs 1, 2, 16 and 24.

The existing TenantWorkspace, TenantGateway, request::Client, Slot, Reads and
Admission modules already have depth. Their
generation, review-epoch and draft rules must survive. Their effective interface
also leaves request tickets, protected-key IO, execution ordering and request-lane
facts in the GPUI implementation. Credential lookup itself is already GPUI-free
in request::Client::send (`src/request.rs:345`). Moving only the state machines would require
React callers to coordinate authority. The old request concurrency is tied to
the TypeScript bridge's process limit (`src/bridge.rs`); it is not a native-SDK
requirement. Retain Admission's interactive-before-read/FIFO behavior,
interactive deadlines measured from submission, queued-cancellation refusal and
per-product rate-limit containment from ADR 16 independently of that numeric cap.

Deepen the Rust application module to own tenant/credential context, request
coordination, configuration-review validity and remote outcomes. React presents
state and operator intent. Feature-specific transitions stay domain-owned;
this does not require one undifferentiated global state machine.

- Before: native view coordinates state machines, slots, protected-key IO and
  effects; request::Client already performs credential lookup outside GPUI.
- After: native Rust owns coordination behind the presentation seam.
- Locality: context and settlement rules concentrate in Rust.
- Leverage: Runtime, Gateway and Harness share authoritative execution rules.
- Real seam: native SDK execution and deterministic transport adapters vary;
  React and Harness are consumers, not alternative authority adapters.
- Deletion test: removing the proposed owner would redistribute the same
  authority rules across callers. Retain the existing deep domain modules;
  avoid an additional shallow pass-through command module.
- Test surface: drive the actual coordinator through tenant switch, credential
  replacement, late/duplicate reply, changed draft, single-use Apply and
  timeout-after-dispatch. An unknown outcome cannot become rollback or replay.

Fresh editor truth is another load-bearing seam: v1 ADRs 1/24 keep immediate
text in native editors behind Form/DraftText/FieldFactory, and ADR 24 rejected
module-owned editable text because of render lag/two sources of truth. V2 must
define fresh React input capture before Review and Apply, invalid-input handling
and draft/review ordering. Moving editor truth into Rust blindly would weaken
the existing design; the selected design must resolve this explicitly.

Preserve v1 ADR intent; their GPUI placement and process-count policy are
reference implementation details, not accepted v2 decisions.

## 2. Shared presentation without learner authority — Strong

Observed: Academy `packages/ui/src/index.ts:7`,
`src/components/shell/app-shell.tsx:42`, `command-palette.tsx:64`,
`mobile-tab-bar.tsx:49`, `rail.tsx:50`,
`src/routes/_authenticated/route.tsx:81` and
`src/components/shell/__tests__/app-shell.test.tsx:8`.

The current UI package is shallow: deleting its re-exports leaves app behavior
almost unchanged. AppShell mounts a catalog-query palette and Clerk-aware mobile
navigation. Rail embeds Academy brand/routes. Shell tests mock Router, Clerk
and tRPC even to render layout. The authenticated web composition also mounts
learner awards, notifications, onboarding and manager navigation.

Topbar's ThemeToggle also persists theme through hosted settings.update
(`src/components/shell/use-theme-preference.ts:17`). The shell barrel exports
AppearanceSync (settings.get) and NotificationsBell (notifications.*), and the
package declares Clerk/tRPC/router dependencies. The desktop theme adapter must
avoid turning a global desktop theme change into a learner-account mutation.

Create one Academy-owned shared presentation implementation for controls,
themes and shell layout. Web and desktop composition adapters supply their
navigation/account actions. Academy account authority and gamification remain
within the Academy feature. Learner views need a separately reviewed extraction:
their hooks currently call hosted tRPC directly; sharing shell is insufficient.

- Before: package re-export points into web app/provider assumptions.
- After: web and desktop consume one shared presentation module.
- Locality: appearance, focus and layout are fixed in one source.
- Leverage: existing Academy web plus explicitly requested desktop consumer.
- Real seam: navigation and theme persistence vary between those consumers;
  no speculative plugin architecture is required.
- Deletion test: deleting the new shared implementation would duplicate real
  presentation behavior in both consumers.
- Test surface: render shared presentation without Clerk/hosted queries; verify
  keyboard/focus/themes and that non-Academy screens trigger no learner effects.
  Retain hosted/account/catalog/award tests in Academy composition.

No authenticated web entry point, manager route or raw content/answer truth is
approved for bundling. Package/source permissions and exact dependency pins
remain extraction prerequisites, not reasons to stop read-only design.

## 3. Native Harness lifecycle and scoped identity — Strong

Observed: v1 `src/harness/transport.rs:116`, `src/runtime_bundle.rs:43`,
`src/harness_auth/exchange.rs:44`, `src/workspace_mcp/exchange.rs:44`;
native engine `codex-rs/cli/src/airs_desktop.rs:55` and
`airs_desktop_runtime.rs:155`; v1 ADRs 30 and 31.

The engine is already Rust and starts with `desktop-server`. Its bounded
bootstrap binds tenant/workspace/credential/project context. V1's package
admission nevertheless mandates Node and the managed CLI; host authentication
also uses TypeScript workers. Removing those workers requires a new native
resource seal, not a full agent-engine rewrite.

The host adds packaged bin/ and managed-cli/ to engine PATH
(`src/ui/harness/connection.rs:772`). The desktop engine instead resets shell
PATH to OS directories (`airs_desktop_runtime.rs:242`), and its base instruction
only mentions airs-cli to prohibit remote mutation/credential reads. The inspected
desktop-server bootstrap path does not launch Node or the managed CLI. This is
source evidence for retaining the engine, not proof a changed bundle works.
Qualify the engine without those resources; pin/manifest and instruction-text
changes may be needed in addition to resealing. Never remove a dependency on
source evidence alone and claim native/package acceptance.

Deepen native lifecycle ownership around the retained engine: scoped bootstrap,
process supervision, grant retirement, resource verification and shutdown.
Concentrate shared native identity mechanics while preserving distinct Academy
learner, Harness inference and MCP grant policies. A generic interchangeable
authentication interface would hide important differences rather than add depth.

- Before: native engine plus worker exchanges and old package requirements.
- After: Rust lifecycle/identity implementation and a newly sealed native engine.
- Locality: process cleanup and grant retirement have one native owner.
- Leverage: reuse the engine and sensitive identity mechanics.
- Real seam: packaged engine versus literal native protocol fixture; device
  authorization, browser PKCE and MCP grants justify shared internal mechanics,
  not one externally interchangeable identity.
- Deletion test: dropping lifecycle ownership would scatter bootstrap limits,
  resource integrity and shutdown obligations into chat callers. Dropping worker
  envelopes mostly removes plumbing; preserve the scoped policy depth.
- Test surface: actual process shutdown, malformed/oversized frames, wrong-context
  bootstrap, mid-turn grant retirement, signature/issuer/audience rejection,
  refresh rotation, sleep/expiry and independent sign-out effects.

Academy device authorization is established in v1 `src/academy.rs:2,501`.
Signature/JWKS validation is a proposed stricter v2 host requirement: v1 ADR 30
explicitly omits it while its worker checks claims. Do not describe the new
signature test as preserved v1 behavior.

Preserve existing project/tool restrictions, protected history, Gateway-only
inference and distinct MCP authority. Exact native OAuth/OIDC selection remains
selected-design work. No current package or authentication qualification is claimed.

## 4. Managed workbench execution admission — Strong

Observed: v1 `src/api_client/scm/mod.rs:86`,
`src/api_client/document.rs:156`,
`crates/rusty-core/src/session/worker/execution.rs:128` and
`crates/rusty-core/src/execution.rs:224`; v1 ADR 27 and v2 C15.

The workbench resolves tenant references at Send **only** when the collection ID
is `prisma-airs-scm` (`scm/mod.rs:169`); other collections use workbench-owned
secret storage. Collection identity comes from editable collection metadata
(`project/discovery.rs:61`, `project/workflow.rs:210`). Static search found no
reservation of that ID; a user/imported file can declare it at the parser level,
but full credential resolution through an imported collection is not runtime
qualified here. The built-in collection also preserves user edits. Its token_url
and request URLs are variables (`scm/mod.rs:321`); together with Live admission,
this source path permits resolution for edited destinations without application
destination binding. This is observed source, not a live exfiltration test.
It then runs a prepared request.
Generation, shutdown and concurrency guards exist. Its transport already blocks
cross-origin redirects when Options.redactions is nonempty (its managed_auth
flag), including workbench-owned secrets. It does not distinguish tenant-managed
credentials from every workbench secret. Network::Live admits destinations
without application destination binding (`execution.rs:29`); the worker's
active<4 concurrency guard is separate from request::Admission.
The inspected path does not connect generic managed-request execution to AIRS
configuration review. That connection is an accepted stricter v2 requirement,
not a finding that v1 violates its own product contract.

V1 automatically resends once after HTTP 401 with a refreshed OAuth token,
regardless of HTTP method (`session/worker/execution.rs:231–273`), subject to
current-request/environment/shutdown checks. This conflicts with accepted v2
no-replay behavior. V2 removes automatic workbench resend; credential refresh
may precede an explicit new Send but does not authorize repeating an operation.
The built-in SCM catalog includes 81 POST/PUT/PATCH/DELETE specifications that
resolve tenant credentials at Send; they require explicit v2 admission, without
claiming all are dedicated GUI workflows or live-qualified requests.

Deepen execution admission in Rust: bind fresh request content, credential
capability, destination and mutation classification to configuration review.
Keep RequestDocument's reconciliation/protection depth separate and preserve
ordinary generic HTTP behavior without managed tenant credentials.

- Before: document snapshot, credential resolution, generic transport.
- After: fresh snapshot, native admission, scoped transport.
- Locality: managed-request authority is decided in one place.
- Leverage: in v2, every path that requests tenant-managed credentials obeys
  native admission, including edited built-in and imported requests.
- Real variation: v1 reserves tenant reference resolution to the SCM collection
  identity while ordinary collections use their own secrets. V2 must replace
  trust in editable collection identity with native-owned credential admission.
- Deletion test: a UI-flag checker would be shallow; the owner earns depth only
  by concentrating request/review/destination validity across every execution path.
- Test surface: imported/edited requests, direct native-command bypass, unknown
  managed endpoints, context changes before dispatch and same-origin redirects
  to mutations. Denial must occur before secrets reach an unauthorized transport.
  Include post-401/lost-response no-resend assertions for managed and generic
  mutations, independent of credential-refresh success. Also test an imported
  collection declaring the SCM identity and an edited token_url/request URL;
  neither metadata nor collection identity can grant credential access in v2.

## SDK source qualification

Read-only inspection fetched nine source/manifest files from accepted scan pin
`ec6230d2a19b9c835c9e4c9487795b38ad538af3` into an isolated temporary snapshot,
without consuming or changing the active SDK worktree. Scanner::sync_scan
(`crates/scm-sdk/src/airs/scan/scanner.rs:65`) constructs the explicit scan request.
`transport.rs:107` defines the injected Transport seam; its ReqwestTransport
disables redirects/retries. These are observed SDK interfaces supporting a real
native/scripted transport seam, not a claim that the Mac integration was tested.
The selected design must preserve RequestOptions/cancellation/outcome contracts
at this pin and qualify native TLS/runtime behavior. Later SDK families require
their own accepted pins, not assumed scan-pin coverage.

## Other exploration priorities

No additional candidate is justified solely to create more modules. Outcome,
freshness and completeness belong to native operation settlement and the
existing domain projections; protected storage/imports are integration obligations
of those selected owners. V1 `src/local_state.rs:1` already owns reset leases
and durable cleanup; `src/harness/history.rs` owns bounded history validation;
`src/settings.rs:191` owns the v1 credential namespace. Preserve that depth.
The selected design must specify independent v2 storage/identity, account-bound
Academy work, reviewed import/rollback and protected-history compatibility.
Typed host tools remain a restricted consumer of operation authority: v1
`src/harness/host.rs`/`tools.rs` provide source evidence, not an Apply permission.
These priorities are integrated into selected design/testing, not dropped.

## Recommendation and remaining design work

Take all four candidates, beginning with Rust operation authority, then shared
presentation, native Harness and workbench admission. This order settles the
first scan's authority before presentation and future features build upon it.
The owner explicitly selected all four through the architecture skill's
candidate-selection question. This is selection, not a claim of design review,
package acceptance or source-license choice.

Framework `server_state.rs:47` depends on Stripe, MySQL, Redis, sessions,
buckets and inference machinery. Importing that whole module would bring
unrelated infrastructure into desktop. Prefer selected patterns and independently
authored desktop implementation; no first-party framework code was copied.

After selection, complete G01's ADRs, concrete interfaces, scoped identity and
storage scheme, Academy packaging plan, exact dependency/command manifests,
parity/live expansion and independent >=9 review. This candidate review does
not satisfy the selected-design review, native qualification or signing.
