# Qualification and implementation handoff

This is a command/ownership contract for C01–C26, not executed evidence. C01
implements the named check runner and targets; missing targets or unavailable
mandatory checks fail the gate. No empty tests or synthetic passing receipts.

## Selected inputs and native strategy

| Input | Initial selection | Qualification owner |
|---|---|---|
| Rust toolchain / target | 1.90.0 / aarch64-apple-darwin | C01 preflight; C02 SDK tests |
| Tauri Rust / JS interface | 2.12.4 / @tauri-apps/api2.12.4 | C01 locked native build |
| Tauri CLI | @tauri-apps/cli2.12.3 | C01 platform bundle/build |
| React / ReactDOM | 19.2.3, matching Academy | C01 both consumers |
| Tailwind / Vite / TypeScript | 4.1.18 / 7.3.0 / 5.9.3 | C01 exact lock/resolution |
| Build Node / package manager | Node24.21.0 / pnpm10.6.5 | C01 build-only preflight |
| Contract generation | ts-rs12.0.1, serde-compat | C01 literal DTO fixtures/regeneration |
| OIDC / OAuth | openidconnect4.0.1 / oauth2 5.0.0 | C04/C20/C22 scoped native contracts |
| Access-token verification | jsonwebtoken11.1.0 | C20 verified claims/signature/JWKS |
| Protected work | chacha20poly1305 0.11.0, full XChaCha20Poly1305 | C02/C07 storage failure tests |
| macOS login Keychain | security-framework3.7.0 | C02 macOS27 API availability/ACL/lock/denial; C26 signed upgrade |
| Native sheet / clipboard | objc2-app-kit0.3.2, objc2-foundation0.3.2, objc2 0.6.3, block2 0.6.2 | C09 private main-thread sheet/ownership tests |
| Native HTTP / trust | reqwest0.13.5 rustls + platform verifier, system-proxy explicit | C02/C15 policy; C04/C20/C22 identity |
| First scan SDK | ec6230d2a19b9c835c9e4c9487795b38ad538af3 | C02 native source/adapter tests |
| Later management SDK | 32cbcb6d9b04478aeca21e767ed6ddb8949ec7bb candidate | C08 exact accepted subset |
| Native Harness | 9331a5839bc6a763d94dee7584746f58bbbe1181 | C19 native protocol/new resource seal |
| Academy shared source | 7f3a28c4082a5bb278bbff5d79fab106c240155a extraction baseline | C01 reviewed shared revision |

C01 resolution (see docs/planning/dependencies.md "C01 resolved inputs"): the
Rust toolchain is corrected to 1.99.0 because Rust 1.90/1.95 proc-macro dylibs
linked at Tauri's 13.0 deployment target fail dyld validation on macOS 27
(evidence/C01/toolchain-correction.json). Frontend build tools follow Academy's
resolved lock so both consumers share one stack: Tailwind 4.3.2, Vite 7.3.6,
Vitest 3.2.6, @vitejs/plugin-react 5.2.0, TypeScript 5.9.3, React 19.2.3.

These choices are not a claim the combined graph compiles. C01 resolves and
commits Cargo/pnpm locks, compatible tauri-build/transitive versions, source and
package integrity hashes; incompatibility blocks C01 and triggers a documented
version correction/review. Later SDK families remain gated at their owning
chunks, preserving first-slice independence. No active worktree becomes a pin.

Build tools run with explicitly selected PATH/toolchain; they never become
installed-app dependencies. The host uses Tauri's singleton Tokio runtime;
Keychain/blocking filesystem work uses its blocking pool. Share the SDK's declared
Transport/RequestOptions/Session seams, not a new request-per-runtime scheme.

Official version/interface references inspected 2026-10-16:
[Tauri](https://docs.rs/tauri/2.12.4/tauri/),
[ts-rs](https://docs.rs/ts-rs/12.0.1/ts_rs/),
[OIDC](https://docs.rs/openidconnect/4.0.1/openidconnect/),
[OAuth](https://docs.rs/oauth2/5.0.0/oauth2/),
[JWT](https://docs.rs/jsonwebtoken/11.1.0/jsonwebtoken/),
[RustCrypto](https://docs.rs/chacha20poly1305/0.11.0/chacha20poly1305/).
Official crate/npm metadata (versions, checksums/integrities, features and declared
MSRV) is retained in docs/planning/reviews/G01/dependency-metadata/. Every selected
version exists and is non-yanked in that snapshot; declared MSRVs fit Rust1.90.
This is availability evidence, not combined compilation. C01 records consumed
archives and exact locks. Native source snapshots of security-framework, AppKit
and reqwest verify the mechanisms described in DESIGN; their hashes are part of
the external review inputs. OAuth/OIDC default HTTP features are disabled in favor
of the common reqwest0.13.5 adapter. Choose jsonwebtoken's explicit rust_crypto
provider during C20 lock qualification, not unchecked decode.

## Command manifest

[checks.json](checks.json) specifies every chunk's fixture target and native/live/
artifact requirements. Commands use repository-relative paths; build-time source
inputs live in immutable `.inputs/` snapshots recorded by dependency manifests.
The controller fingerprints their actual consumed bytes separately, even when
`.inputs/` is Git-ignored. It cannot rely on a mutable source path or only HEAD.
Never install/build inside `.inputs/`: shared-prepare copies verified bytes into
`.work/academy` and rejects symlink escapes, .git metadata and unrecorded inputs.
Scratch outputs/node_modules stay outside the source fingerprint; source and
input hashes are verified before/after commands, including the v2 .git/config.

Install with --ignore-scripts. C01 creates a reviewed exact-version/script-hash
shared-lifecycle.json allowlist for only required dependency setup (such as the
consumed esbuild binary installer), run explicitly in scratch by shared-lifecycle.
Unknown package scripts fail; Academy root prepare/Husky/git-keepalive never run.
The Excalidraw asset-copy script is explicitly reviewed and invoked in scratch;
no root prebuild, API server, migrations or deployments run.

C01 introduces vitest.presentation.config.ts in the reviewed Academy revision:
jsdom/DOM setup, explicit packages/presentation and changed shell tests, no root
globalSetup/PGlite template, hosted providers or environment credentials; network
attempts fail. The existing root suite is not claimed hermetic or required here.
The scoped suite covers provider-free shell/theme plus web composition regressions
(app-shell/topbar/appearance-sync/notifications-bell). Run shared_source_commands
in C01/C05/C06 and whenever consumed Academy bytes/revision change. C05/C06 extend
the same scoped config with learner-export suites and both consumers' regressions. Typechecking and static web
build use documented synthetic build-only env inputs, never .env/user credentials;
any hidden DB/Clerk/network dependency blocks C01 until removed or isolated.

Each command uses checks.json's environment_contract: clear inherited variables,
preserve the real HOME unchanged for standard tool/CLI profiles, select explicit
preflight PATH/toolchain and synthetic build values, reject undeclared overrides
and .env loading. Fingerprint public effective values; private review/live/signing
credentials are explicitly sourced and never logged/hashed into public evidence.
Scratch source preparation excludes .env files. No build/test imports tenant/IdP
credentials merely because they exist in the parent shell.

The expected CLI is `cargo run --locked -p xtask -- <subcommand>`. Implement it
in C01 with a strict schema; no shell strings supplied by review output. It runs
argv arrays from the approved manifest, records each exit code/output hash and
checks actual expected targets. Common checks are Rust formatting/clippy,
generated-binding diff, React typecheck/build/Vitest and the chunk's integration
tests, desktop-host adversarial targets and fixture-enabled clippy. Named frontend suites bind C01 invoke/shell, C03 Scan, C09 fresh Review/
Apply, C14 Save and C15 Send to the production handlers; absent/zero-test targets
fail. Tests exercise invalid input and click-before-debounce, not copied builders.
Do not claim these commands run before the runner/targets exist.

check validates manifests and common checks; test executes named Rust/frontend
fixtures; native builds/launches via native-prepare then requires actual acceptance
records. review freezes those inputs/evidence, writes an immutable request
packet and delivers it to Codex (D026) through the preferred thread or the fresh
read-only `codex exec` fallback with the prompt on stdin and a strict output
schema; pin executable/version/hash, and verify source/dependencies/checks
unchanged afterward.
C01 verifies every reviewer flag against the actual pinned CLI's --help/version
and records executable hash; unsupported flags fail rather than weakening review.
The three workspace members explicitly declare test-fixtures; clippy uses package-
qualified feature names so none is silently skipped. Substitutions are approved schema JSON, fixed frozen directory and valid chunk IDs,
never executable reviewer text. complete is a read-only gate validator followed by
an append-only completion receipt; it cannot fabricate results or advance failed
gates. package --kind preview permits only completed advertised chunks and visible
unavailable features; release-candidate requires C01-C25 and the C26 artifact
checks. Neither command uploads/releases automatically. Artifact validators live
in xtask, keeping desktop-core independent of Tauri/package tooling.

Fixture native launch uses `test-fixtures` only in a distinct fixture app/data/
credential identity, literal loopback transport, no DNS/proxy and synthetic stores.
Production rejects fixture flags/endpoints and includes no fixture command grants.
Shipping feature flags are default implemented features; no `--all-features`.
The controller binds exact Cargo features/Tauri config/target/build flags per run.
Fixture Rust targets and native-prepare explicitly enable test-fixtures in the
manifest. Shipping artifact_build uses no fixture feature; compile/configuration
checks prove fixture endpoints and commands are absent.

`native-prepare` builds/launches the fixture app and records its identity/hash;
CUA-supported native interaction or a recorded operator procedure then supplies
T09 evidence. It does not fabricate keyboard/focus/VoiceOver acceptance from a
CLI exit code. Each changed UI chunk checks relevant T09 paths; C24 repeats the
cumulative matrix. Tests of Rust/Tauri handlers are additional evidence.

Review execution uses the actual Codex protocol in review-protocol.md. The controller
checks all five >=9 scores plus no required fixes/missing evidence and matching
source/dependency/check hashes. A separate reference validator bootstraps C01's
controller review. Native/live/artifact results are distinct fields, never
inferred from test count, reviewer score or an old compatible-looking receipt.

## First-slice behavior checks

- C01: actual Tauri shell and shared web/desktop presentation; no learner account
  side effects or startup product network; controller negative/adversarial receipts.
- C02: classic SecKeychain APIs are deprecated; verify continued availability and
  behavior on macOS27. If unavailable, block the gate and review a signed
  data-protection-Keychain/entitlement adapter and migration contract; never weaken
  ACLs or silently store plaintext. Explicit session-only use remains a separate
  supported choice, not an undisclosed persistence fallback.
- C02: exact SDK source build and injected transport contracts on Mac; scan-only
  Keychain/session mode, denied/locked storage, endpoint review/import and switch/
  rotation/cancel races through the native authority interface.
- C03: production Tauri→Rust→SDK sync-scan fixture with literal external method/
  path/body/header expectations; denied/unknown/partial/timeout/cancel, stale tenant
  reply and post-dispatch uncertainty; offline local work and explicit retry.
  Assert pinned SDK mappings: send transport errors are Unknown even for connect
  failure, 3xx Destination/Rejected, 4xx Rejected and 5xx Unknown; no guessed
  NotSent merely from a generic transport error.

A required test unavailable on this host is blocked, never silently skipped.
Toolchain preflight is distinct from a successful application or native test.

## Budget method

Register numeric core budgets before C03 acceptance runs. Calibration runs are
separate and labelled exploratory; they cannot be re-labelled as acceptance.
Initial candidate host is Mac17,3 / Apple M5 /24GiB/macOS27.0.1 (26A434).
Measure 20 cold process launches, 20 warm launches and 20 deterministic 100KiB
synthetic scan interactions; record median/p95/max plus baseline/app hashes.
Process-tree memory includes WebView processes. Measure unsigned app bytes and
DMG bytes separately; final C26 repeats against exact signed installed bytes.

Proposed pre-registration limits for shell/scan, to freeze before acceptance:
cold usable shell p95<=2s; warm p95<=1s; input-to-paint p95<=50ms; steady idle
process-tree resident memory<=350MiB; scan peak<=500MiB; first-slice app<=150MiB
and DMG<=100MiB. These are targets, not observations. A change retains the old
failure/budget, rationale and required independent acknowledgement. Later Academy,
large collections/HTTP, Gateway, Harness and MCP workloads register separate
budgets at their introducing chunks; no retroactive C24 fitting.

## Hosted identity registrations

| Flow | Reuse/new registration decision | Responsible operator / prerequisite |
|---|---|---|
| Academy device | Prefer existing public desktop client from hosted /api/desktop/config; verify academy:learn scope/client restrictions | Owner-designated Academy/Clerk operator; staging config/read/refresh/revoke in C04/L11 |
| Harness company SSO | Prefer existing public PKCE S256 client with ephemeral 127.0.0.1 callback, iss response and Gateway audience | Owner-designated IdP/Gateway operator; exact callback/audience contract in C20/L16 |
| Workspace MCP | Follow approved Gateway-discovered OAuth registration/authorization; separate grant from inference | Owner-designated Gateway/IdP operator; client/redirect/scope contract in C22/L17 |
| Management | Tenant's explicit confidential management registration, separate from scan/inference | Designated tenant administrator; least-privilege management access in C08/L02 |

V2 bundle identity alone neither requires nor authorizes a new OAuth registration.
Discovery/configuration source is not proof a live registration is ready. Record
reviewed reuse or an explicitly authorized new staging registration before its
consuming live test. No production registration/deployment change is performed
by this design. Exact live accounts, scopes, reversible resources and budgets
remain the explicit C25 execution manifest, not credentials committed here.
