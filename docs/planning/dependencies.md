# Dependency and source gates

No dependency versions have been selected or installed in this v2 checkout.
The following are inspected source inputs and gates, not a claim of ready
release dependencies. C01 records immutable source/package hashes and lockfiles.

## Source ownership

| Input          | Inspected revision                       | Ownership/use                                           |
|----------------|------------------------------------------|---------------------------------------------------------|
| cdot-framework | e3553b27a3d6bf457a2e585c5ae479c7db89a17b | Selective patterns/modules after provenance evaluation  |
| Desktop v1     | 74754c02b19d87187b6dca81f64550a669b0f841 | Behavioral/contracts reference; preserve checkout       |
| Academy        | 7f3a28c4082a5bb278bbff5d79fab106c240155a | Shared React source; hosted learning authority          |
| Native Harness | 9331a5839bc6a763d94dee7584746f58bbbe1181 | Desktop-capable native engine/protocol input            |
| SDK Phase 3    | ec6230d2a19b9c835c9e4c9487795b38ad538af3 | Accepted scan subset; native/live qualification pending |
| SDK Phase 4    | 32cbcb6d9b04478aeca21e767ed6ddb8949ec7bb | Accepted management/DLP/IAM subset; qualify exact uses  |

Do not consume an active neighboring worktree via an unpinned path and later
claim the accepted commit was built. Obtain a clean revision-specific source
snapshot/package without secrets or ignored local configuration. Manifest both
its origin revision and actual source/package hash. A dependency update reopens
the affected contract/native/artifact checks and review. Never fabricate SDK
method parity from a similar TypeScript method name.

## Required gates

- **Distribution terms:** G01 records owner-approved v2 source/binary terms,
  Academy redistribution permission and every framework-derived import's
  disposition under [distribution.md](distribution.md). No extraction before
  clearance; C26 verifies notices/terms against exact shipped inventory.

- **Provenance:** evaluate each direct import's actual license/notice and allowed
  usage. cdot-framework is not blanket MIT and its billing-retention condition
  cannot be erased by renaming. Prefer independently implemented patterns where
  direct copying is unsuitable. Academy sharing is owner-directed; establish
  package ownership and third-party notices rather than inventing a license.
- **Academy packaging:** React19/Tailwind4/Vite7 are observed in Academy, while
  the framework uses React18/Tailwind3. Select a compatible pinned desktop stack
  and one shared presentation source. The existing UI package re-exports app
  source and is not a ready independently reusable full learner app.
- **Shared-source changes:** propose the smallest extraction that supports actual
  web/desktop consumers. Keep Academy web behavior and hosted contracts stable;
  run both projects' affected tests. Pin the reviewed shared revision/package
  before completing its consuming desktop chunk.
- **SDK scan:** ec6230d2's accepted subset supplies synchronous scan. It requires
  explicit scan credentials; management OAuth is separate. Accepted Linux
  reports do not establish macOS ARM64 support. Qualify the native compiler/TLS
  dependencies, runtime integration, scanner contracts, cancellation, sanitized
  errors, and unknown outcomes in C02–C03.
- **SDK administration:** 32cbcb6d's accepted Phase4 is a candidate input for
  C08–C12. Map every actual operation, option, pagination bound, and error to
  accepted source/tests. SDK family labels are insufficient proof of parity.
- **SDK later families:** Phase5 Red Team/Model and Phase6 Gateway were developing
  or pending during inspection. Their exact accepted pins are prerequisites to
  C13/C16–C18. Do not consume unfinished work or substitute mocks as completion.
  Stable v2 must qualify the SDK subset it actually uses, including native/live
  evidence; it need not claim every unrelated SDK method is complete.
- **Harness:** retain the desktop-capable native engine/protocol after source and
  package checks. Do not substitute an older canonical checkout or npm launcher.
  Rebuild/reseal v2 resources to omit Node/managed CLI where proven unnecessary;
  old v1 resource hashes do not attest the changed v2 bundle. Preserve inference
  and MCP routing/context/tool restrictions.
- **Native host:** select pinned Tauri2/Rust/toolchain dependencies compatible
  with macOS ARM64 and the chosen SDK. Resolve one application-owned async
  runtime strategy; do not create an independent runtime per request.
- **Identity:** specify independent v2 bundle/display/data/Keychain identities in
  G01. Test v1/v2 coexistence, reset, uninstall, import, and rollback. Do not
  silently reuse a v1 namespace or change the learner's remote account.
- **Live environment:** identify designated test tenants/accounts, scopes,
  reversible resources, budgets, and cleanup/readback before C25 execution.
  Prod configuration discovery is not mutation authorization. Missing access is
  a real dependency gap; never report skipped tests as passed.
- **Hosted OAuth registrations:** G01 maps reuse/new-client/redirect/scope needs
  for Academy device authorization, Harness company SSO and MCP. Responsible
  Academy/Gateway/IdP operators own authorized staging/configuration changes and
  deployment. V2 identity alone neither proves a new registration is required
  nor authorizes production changes. Resolve before the consuming live tests.
- **Signing:** identify an authorized Developer ID, provisioning/entitlement
  requirements, notarization access, and a native acceptance host. Access secrets
  only at use time through approved storage, never committed plan variables.

## Official host references

Tauri supports Rust commands and async messages between presentation and host;
the selected command/module interface still requires G01 design and review.
[Calling Rust](https://v2.tauri.app/develop/calling-rust/)

Window/WebView capability grants require deliberate scoping; renderer grants do
not replace Rust's application authority checks.
[Capabilities](https://v2.tauri.app/security/capabilities/)

DMG distribution and macOS signing are separate from successful compilation.
[DMG packaging](https://v2.tauri.app/distribute/dmg/),
[macOS signing/notarization](https://v2.tauri.app/distribute/sign/macos/).
