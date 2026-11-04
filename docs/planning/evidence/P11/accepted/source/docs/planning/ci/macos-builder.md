# Internal Apple Silicon builder

This branch validates one production compile on Jadzia's repository-scoped
`airs-jadzia-arm64:host` runner. It does not change application code, qualify a
release, reopen a chunk, or execute native gates. C26/P08 remains open.

The workflow accepts only pushes to `infra/macos-builder-20261103` in
`cdot/prisma-airs-desktop-v2`. Review this branch before pushing it: that push
starts the build. There are no pull-request, tag, release or manual-dispatch
entry points. The runner must stay restricted to trusted maintainers' code.

The helper requires the actual triggering commit, a clean checkout and ancestry
from accepted application commit `1241916e46156b6fe821f98b93bf281c255a7cbe`.
Every difference from that base must be an addition of one of the five declared
CI files. Any application, existing controller, lockfile or resource change
stops the job. Provenance records the actual build commit and accepted base;
it does not claim that an earlier qualified application was rebuilt identically.

## Inputs and host setup

Jadzia uses the already verified isolated Node24.21.0/pnpm10.6.5 profile plus
Rust1.99.0 and Xcode. The job makes ignored local `.tools/bin` links so the
controller's selected PATH finds those tools. No global defaults or services
are changed. Public Cargo and pnpm downloads use dedicated runner caches;
dependency lifecycle scripts are disabled. Only prepared Academy TypeScript
is compiled: Academy server, database and root preparation scripts never run.

Academy is fetched over existing read-only, noninteractive SSH at exact commit
`532809aa4bb2e1484e11ca9f7a414328260c7901`, then `fetch-inputs` and
`shared-prepare` enforce tree SHA256
`73e1c80889af89311b374808fce1f99386ced2fe95bc3a54429c40a72ce5530d`.
An operator may instead preprovision the immutable source directory at
`~/.local/share/airs-desktop-forgejo-runner/inputs/<tree SHA256>/academy`.
The same shared-prepare hash check is mandatory for that fallback.

Git LFS materializes only `resources/harness/darwin-arm64.tar.gz`, expected size
113497192 and SHA256
`10729706be0bf2ec2bae1d21155fe866b047960e73881583397887c083fce173`.
Historical qualification executables are left as LFS pointers. Alternatively,
put that exact archive at `~/.local/share/airs-desktop-forgejo-runner/inputs/<archive SHA256>/darwin-arm64.tar.gz`.
Both fetches require existing read-only Forgejo access; neither registers a
runner or changes credentials. BatchMode forbids interactive authentication.
Checkout uses a commit-pinned official action with credential persistence off.
Jadzia has no Git LFS executable: the exact Harness fallback was therefore
preprovisioned and verified on2026-11-03. Its host receipt is
`~/.agent-workflows/airs-runner-setup-20261102/builder-input-cache.json`.
Academy's exact-commit fetch was verified with SSH agent access disabled;
`academy-input-access.json` records that host preparation check.

## Build and result

`ci_macos_build` reuses the reviewed Harness archive/inventory/pin verifier, then
builds the frontend and invokes production Tauri with `--ci --no-sign`, target
`aarch64-apple-darwin`, `--bundles app`, the Harness resource overlay, and Cargo
`--locked`. It enables no fixture feature or fixture identity. Cleared child
environments exclude action tokens, Apple/notary/updater signing values,
SSH agents and compiler overrides. Existing key authentication is used only
by noninteractive input-fetch subprocesses.
Personal npm config is disabled and Cargo uses a separate public-build cache.

The resulting app is checked for production identity `io.cdot.airs-desktop`,
version1.0.0, native arm64, and the exact bundled Harness hash. Its archive,
canonical app-tree/executable hashes, tool identities, source/lock/input hashes,
run identity and actual timestamps are retained for seven days as an internal
Forgejo Actions artifact under `target/ci-macos-build/`.

The outer app is unsigned/unnotarized. The Harness retains its historical
signature in its immutable input bytes; no signing key is accessed. This job
does not create a DMG, launch or install the app, call product endpoints, use
Keychain, sign, notarize, publish releases, mirror assets or record C26 evidence.
It is an internal build candidate, not a redistributable release.

## Verification scope

The change affects CI dispatch, input provenance and child-process isolation.
Focused Python tests cover unauthorized ref/event/commit rejection, rejecting
application deltas, pinned-input drift and stripped credential/override values.
Rust formatting and Python/YAML checks are appropriate preparation checks.
The first approved CI run compiles this new adapter and the production app.
No full Rust suite or native end-to-end rerun is warranted by CI-only additions;
the original accepted evidence and its bindings remain unchanged.
