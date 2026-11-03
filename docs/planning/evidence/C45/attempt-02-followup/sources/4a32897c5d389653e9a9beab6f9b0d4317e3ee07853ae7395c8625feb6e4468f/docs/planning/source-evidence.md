# Source evidence for planning

Read-only inspection snapshots. Recheck revisions and local modifications before
using these facts as implementation inputs. No application endpoints, credentials,
builds, release operations, or live acceptance tests were exercised for this
interview.

## cdot-framework

Checkout: `/Users/cdot/development/redtail/ampersand-services`.
Inspected HEAD: `e3553b27a3d6bf457a2e585c5ae479c7db89a17b`.

- The repository is an application monorepo, with Rust HTTP modules/workers and
  React browser presentation. Its actual upstream Tauri Rust application lives
  separately. See [the repository overview](/Users/cdot/development/redtail/ampersand-services/README.md).
- The main [server state](/Users/cdot/development/redtail/ampersand-services/crates/service/web/redtail_web/src/state/server_state.rs)
  depends on media-product infrastructure including Stripe, MySQL, Redis,
  storage buckets, and inference integrations.
- [Bootstrap configuration](/Users/cdot/development/redtail/ampersand-services/crates/lib/bootstrap/src/bootstrap.rs) assumes
  repository environment files, containers, and Kubernetes metadata. It is not
  automatically an appropriate desktop configuration module.
- Existing frontend Tauri libraries include wrappers for commands supplied by a
  different desktop host. They do not supply that host implementation.
- The [root license](/Users/cdot/development/redtail/ampersand-services/LICENSE.md) explicitly contains commercial/competition
  restrictions and a fork condition retaining community/donation links and paid
  model functionality. The frontend package's MIT metadata is not a blanket
  repository license. D001's no-billing requirement makes the exact reuse strategy
  material; do not presume a blanket right to distribute a stripped fork.

## Prisma AIRS Desktop v1

Checkout: `/Users/cdot/development/cdot65/prisma-airs-desktop`.
Inspected HEAD: `74754c02b19d87187b6dca81f64550a669b0f841`.

- GPUI-CE presentation; Rust application logic; TypeScript SCM SDK workers.
- `src/workspace.rs`, `src/request.rs`, and
  `docs/adr/0001-tenant-workspace-state-machine.md` document tenant-scoped state,
  draft/review versions, cancellation, and rejection of stale completions.
- `src/screens.rs` distinguishes working workflows from unavailable or preview
  behavior. Exception Patterns includes synthetic behavior, not verified live
  suppression.
- `src/settings.rs`, `src/local_state.rs`, and `src/cli_tenants.rs` expose
  credential references, Keychain storage, saved cases, controlled reset, and
  review-before-import behavior relevant to migration requirements.
- Harness identity, Academy identity, tenant credentials, and MCP grants are
  distinct. Replacing the SCM SDK does not itself replace the Harness runtime.
- Existing signing/notarization scripts are requirements reference for actual
  installed-artifact qualification, not permission to run a release now.

### Harness runtime clarification

The agent engine is already a pinned native Rust sidecar, not a Node agent
engine. The desktop-capable source checkout is
`/Users/cdot/development/cdot65/prisma-airs-harness-desktop-mcp`, inspected at
`9331a583` (the desktop pin). Its `codex-rs/cli/src/main.rs` dispatches `desktop-server`
directly. Do not assume an older canonical Harness checkout provides the same
desktop host integration.

The current desktop bundle nevertheless contains Node and a managed Prisma AIRS
CLI/TypeScript SDK. The host also runs TypeScript authentication/MCP workers and
AIRS tool adapters. Replacing the SCM bridge alone does not remove these runtime
dependencies. A Node-free shipped product would need separate authentication,
Academy transport, host tool adapters, and package qualification work. Preserve
the distinction between a bundled product runtime and user-approved external
development tools.

The desktop bundle verifier currently requires Node and the managed CLI. That
does not establish that ordinary native Harness chat needs them: the desktop
starts the Rust engine directly and uses typed host tools. Investigate whether
the managed CLI can be omitted after worker replacement and package resealing;
do not automatically add a full CLI rewrite to the v2 scope.

The pinned desktop engine disables its standalone skills/plugins, remote exec,
networked local commands, and plaintext history, and uses in-memory thread
storage for the host-owned integration. The v2 plan must evaluate and preserve
the relevant host authority rules rather than silently substituting the
standalone CLI's defaults.

## Rust SCM SDK

Accessible through existing SSH alias `airs.coder` at
`/home/cdot/development/cdot65/scm-sdk-rs`.
Initial inspected HEAD: `ec6230d2a19b9c835c9e4c9487795b38ad538af3`, the accepted
Phase 3 snapshot. Initial inspection observed staged Phase 4 work. A later
read-only inspection during this interview found Phase 4 committed and accepted
at `32cbcb6d9b04478aeca21e767ed6ddb8949ec7bb`, with active uncommitted Phase 5
work. No agent changed the SDK. These are separate snapshots; do not treat the
initial staged-work warning as the current state.

- `docs/phase-status.json`: Phases 1–4 now accepted with actual Claude reviews at
  9/10; Phase 5 in progress; Phases 6–8 pending at the later inspection. This is
  not a released, fully qualified SDK.
- `contracts/native-operations.json` at the accepted Phase 3 snapshot records
  7 implemented / 448 baseline methods and 0 live-verified records. The initial
  Phase 4 worktree snapshot recorded 99 implemented, 349 pending, 97 complete
  option-parity records, 2 partial records, and 0 live-verified records. Do not
  merge these counts or present developing capability as accepted functionality.
- Rust 2024, MSRV 1.88, Tokio, reqwest/rustls, injected transport support, and
  explicit configuration. No Node runtime or desktop presentation dependency.
- The application owns runtime integration, persistent credentials, interactive
  login, OS protected storage, and application enforcement policy.
- Native execution evidence currently covers Linux x86_64 musl. macOS
  qualification must be established, not inferred from compilation elsewhere.
- Select and pin accepted capability subsets for desktop slices. Do not assume
  all baseline operations exist or require every operation before proving the
  first desktop workflow.

### First-slice dependency evidence

Accepted scan snapshot: `ec6230d2a19b9c835c9e4c9487795b38ad538af3`.
`docs/reviews/phase-03-attempt-02.json` records score 9 and unchanged accepted
fingerprint `18bcfea7ac50002482005b7cd811c0fa25111ea38c129d058b05a63efcf5176d`.
Its retained all-feature Rust 1.99 check report records 139 passing tests; that
report establishes Linux x86_64 musl execution, not macOS qualification.

`crates/scm-sdk/src/airs/scan/scanner.rs` supplies synchronous scan, async batch
submission, scan results, and reports. D009 can start with synchronous scan alone
(`sync_scan`, `POST /v1/scan/sync/request`). Scanner credentials are an explicit
scan API key and/or scan bearer token, not implicitly management OAuth credentials.
The supplied bearer token has no automatic token-provider lifecycle at this pin.

`Session`, `RequestOptions`, and structured errors support bounded cancellation,
timeouts, and `RemoteOutcome::{NotSent, Rejected, Unknown}`. Scan submissions do
not automatically replay. Cancellation does not establish that remote work did
not occur. Unknown verdict/action values survive decoding and must remain visibly
unknown in the desktop rather than being treated as allow.

Accepted Phase 4 snapshot: `32cbcb6d9b04478aeca21e767ed6ddb8949ec7bb`.
Its score-9 receipt binds fingerprint
`50fd6e8a79bbcb21749754d400aeb2e49f35e9a5c1077215e79db6ede8146ce1`.
This separate dependency covers management, DLP, IAM, and bounded pagination.
Model Security/Red Team depend on Phase 5; full Gateway on Phase 6; broader
AgentGuard/realtime/export closure on Phase 7; stable/live/platform qualification
on Phase 8. Harness, Academy, and desktop packaging are application work outside
the SDK. Recheck accepted revisions before selecting a final dependency pin.

## AI Security Academy

Checkout: `/Users/cdot/development/redtail/ai-security-academy`.
Inspected HEAD: `7f3a28c4082a5bb278bbff5d79fab106c240155a`.

- React 19 SPA with Vite, TanStack Router, React Query/tRPC, and Tailwind 4.
  Its backend uses Express, Postgres/Drizzle, Redis, and hosted authentication.
- cdot-framework currently uses React 18 and Tailwind 3. Shared UI reuse requires
  deliberate dependency, stylesheet, navigation, and shell integration.
- `AGENTS.md` requires answer truth, lab evaluation, final assessment timing,
  access decisions, and credential awards to remain server-owned.
- Bundling learner presentation is a feasible direction to investigate. It does
  not establish that the whole backend belongs in a desktop bundle or that the
  browser authentication flow works unchanged in an embedded WebView.
- `src/components/auth-trpc-provider.tsx` assumes Clerk browser session tokens
  and a relative `/api/trpc` endpoint. The authenticated route imports manager
  navigation and web account controls; a desktop learner entry requires explicit
  route/provider selection.
- `/api/desktop/config` and `src/server/services/desktop-auth.ts` already define
  public-client device authorization, desktop learner scope, and an explicit
  learner procedure allowlist. `src/server/trpc.ts` verifies desktop OAuth
  separately from browser sessions. Live deployment configuration was not checked.
- Lesson retrieval passes through `units.get` and access enforcement. Client-safe
  projections remove private truth. `src/content/README.md` explains why shipping
  raw authored content could disclose answer keys and private evaluation rules.
- `src/server/services/desktop/` supplies client-safe downloaded bundles,
  account/revision metadata, seven-day leases, a 7 MiB bundle limit, versioned
  drafts, and submission receipts. Grading and award settlement remain hosted.
  Offline video/media coverage is not established by bundling presentation.
- Targeted runtime/manifest inspection found no financial payment subsystem.
  Academy does have earned cosmetic credits and a requisitions shop. D006 retains
  learning gamification only within Academy. Learning grants/prerequisites are
  distinct from financial billing.
- `packages/ui/src/index.ts` re-exports the application's presentation modules;
  it is not an independent complete learner application. `AppShell` contains
  reusable layout, but its rail branding/navigation, command palette, mobile
  navigation, and notifications still assume Academy. The authenticated route
  installs Academy gamification and account effects. D005 requires evaluating
  these seams rather than mounting that route as the whole desktop root.
- No root LICENSE/COPYING or license field was found in root/shared/UI package
  manifests. They declare `private: true`. This does not decide reuse rights;
  establish ownership/permission and third-party notices for distribution.
