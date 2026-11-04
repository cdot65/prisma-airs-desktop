# Preview, live qualification, and signed-package contract

This is future execution guidance. No package has been built or signed, no live
acceptance was run, and no application release is authorized by this document.

## Preview and stable definitions

Preview DMGs may demonstrate accepted chunks after the architecture gate. Name
implemented/missing features and evidence limits clearly. A preview cannot become
stable by changing its label. Keep v1 available throughout qualification.

Internal previews may be development-signed and unnotarized and remain local,
clearly labelled nonredistributable. Externally shared previews require Developer
ID/notarization/installed declared-scope checks through the package pipeline;
they cannot satisfy stable parity or unlock C26 completion early.

Stable requires the full agreed working-v1 parity ledger; all chunk and cumulative
reviews meeting D019; no mandatory failed/blocked/unrun checks; designated live
acceptance; and exact signed/notarized/stapled installed-DMG acceptance.

## Controlled live manifest before C25

Record named accounts/tenants/environments, endpoint and credential-capability
bindings, entitlements, permitted operations, disposable resource names, expected
effects, limits/budgets, explicit irreversible exclusions, and cleanup/readback.
Scope is the owner-approved expanded [live matrix](live-checks.md), not whatever
tests are accessible at C25. Required/not-required reasons and separate blocked
access status are frozen. Any reduction needs a recorded owner decision/review.
Use least-privileged designated test resources. Production discovery/config access
is not authorization for configuration mutations, paid inference, or examinations.
If an authorized test environment does not exist, report the dependency gap and
request the needed designated scope rather than falling back to production.

Every mutation has preparation/review and outcome handling; uncertain outcomes
stop automatic progression and require reconciliation. Live tests cover required
positive/deny/error/recovery behavior, not just a happy-path screenshot. Record
real request/trace identifiers privately/redacted as appropriate. A mock refusal
does not close a required live-denial check.

Confirm cleanup and preserve receipts. Avoid issuing real certificates or running
real adversarial/model scans merely to test UI; use designated synthetic learning
and test tenants with the proper authority. Unknown/missing evidence is blocked.

## Build/signing inputs

Freeze source, dependency hashes, generated bindings, Cargo/npm locks, target,
feature/build flags, resource inventory, notices, entitlements, bundle identity,
version and test-mode policy. Runtime uses bundled native code and React assets;
it never installs/downloads Node/Bun/TS workers or requires a developer checkout.
Approved [distribution terms/permission](distribution.md) are a release
precondition; their source/binary terms and notices must match shipped inputs.

Retain the native Rust Harness binary/protocol pin, rebuild/reseal changed v2
resources, and sign nested executables/libraries before their containing bundle
as required by the chosen toolchain. Use the authorized Developer ID and hardened
runtime/entitlements. Secrets are supplied at use time through approved storage.
Record pre-sign and post-sign bytes and notarization submission/result, staple,
and final DMG hash. Do not reuse v1's old resource hash after changing its contents.

Validate final signatures/notarization/Gatekeeper, including installed nested
native components. Signing success alone is not installed acceptance. Official
references: [macOS signing](https://v2.tauri.app/distribute/sign/macos/) and
[DMG distribution](https://v2.tauri.app/distribute/dmg/).

## Exact installed-artifact evidence

Mount the final DMG, drag/install, preserve quarantine, and launch as a fresh
nondeveloper user on a named supported Mac/OS. Use private synthetic/test accounts.
No source checkout, Rust, npm, Node/Bun, or managed CLI installation is required.
Observe process/network/resource behavior to prove no hidden backend runtime or
download fallback. WebKit processes and user-started browser authentication are
expected; they do not count as Node backend execution.

Exercise core scan, learner sign-in/content/offline work, Gateway/Harness native
engine paths and exact final-artifact controlled live smoke. Verify native menus,
keyboard/focus/VoiceOver and protected store behavior on the signed installed app.
Check missing/tampered runtime rejection, reset/uninstall, v1 coexistence, reviewed
imports, fresh authentication, and user-work preservation.

Map this evidence to the final signed/stapled DMG and installed inventory hashes.
The mandatory final-byte smoke set is F01–F09 in [live-checks.md](live-checks.md).
Fixture-build or pre-sign captures cannot claim the shipping bytes were tested.
Test-mode controls must not expose fixture endpoints/authority in the shipping
configuration; compilation flags and package inventory attest that distinction.

## Manual update and rollback

Start with manually installed signed DMGs. Document version identification,
installation, backup/export where needed, data/schema compatibility, replacing
the v2 application, and returning to the retained previous supported v2 version.
Test both forward update and rollback with real synthetic user work; no silent
data loss, v1 namespace changes, credential/session resurrection, or old approvals.

For the first stable v2, use an explicitly approved versioned precursor preview
or rollback-test build to exercise forward/backward installation and data handling;
record its status and hashes rather than inventing a previous stable release.
The precursor source is C24's approved versioned snapshot; C26 produces its
signed/notarized rollback-test package alongside the candidate. This is a local
test artifact, not an implicit public release or earlier stable version.
Subsequent releases use the retained previous supported stable v2 as appropriate.

Any storage change must declare its backward readability or provide a verified
rollback/export path before release. An unsupported downgrade must be detected
before changing data. An in-app updater is a later explicitly designed milestone.

## Delivery and publication

Deliver the verified DMG, hashes, redacted acceptance report, known limitations,
manual update/rollback instructions and reviewed source commits. Normal Git
pushes target only the owner-specified v2 repository. Do not force-push, replace
v1 releases, promote v2 by default, or publish application release assets unless
the active owner instruction includes that action.
