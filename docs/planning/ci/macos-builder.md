# Internal Apple Silicon builder

The original RC1 admission and a disabled P11 draft share one unsigned
production-build recipe on Jadzia's repository-scoped
`airs-jadzia-arm64:host` runner. Neither lane qualifies a release, reopens a
chunk, runs native gates, signs or publishes. C26/P08 remains open.

## Original RC1 lane — unchanged admission

Pushes to `infra/macos-builder-20261103` in
`cdot/prisma-airs-desktop-v2` retain the original contract: exact triggering
commit, clean checkout, ancestry from accepted application commit
`1241916e46156b6fe821f98b93bf281c255a7cbe`, and a nonempty delta consisting
only of additions of the five declared CI files. Existing-file modifications,
application changes, lockfiles and resource changes are rejected. Published
RC1 source, tags, assets and qualification are unchanged.

## P11 candidate lane — disabled until final phase acceptance

The new `infra/macos-candidate-p11-20261104` branch is preparation only.
Its workflow condition contains a literal `false` for this branch, and
`P11_ACCEPTED_SOURCE` plus `P11_ACCEPTED_TREE` are deliberately `None`
in the controller. Direct controller admission also stops on these missing
pins before input materialization, dependency installs or compilation.
No draft push or dispatch is authorized by this preparation.

`62580dec9287d05b7a4aae03ca0625247f28542c` is the provisional qualified
ancestry bound only. It is never used as a default accepted source.
After the independent P11 chunk and phase gate, root supplies the real
accepted commit and its Git tree. The build commit must descend from that
exact pin; that pin must descend from the provisional ancestor. A different
resolved commit/tree, dirty checkout, wrong event/ref/repository or mismatch
with the immutable triggering SHA stops admission.

Only a nonempty delta within these five paths is allowed above the final
accepted application source:

- `.forgejo/workflows/macos-builder.yml`
- `crates/xtask/examples/ci_macos_build.rs`
- `tools/ci/macos_builder.py`
- `tools/ci/test_macos_builder.py`
- `docs/planning/ci/macos-builder.md`

P11 permits additions and reviewed modifications because these files already
exist at its base. Deletions, renames, symlinks, file-type/mode changes and
every product-source/input change are rejected. This allowance does not
change the original RC1 additions-only contract. Full CI delta hashes and
both the accepted application commit/tree and actual build commit are
recorded in provenance; no earlier qualified binary is claimed as this build.

Activation is one reviewed follow-up in this isolated branch:

1. Complete P11 reviews/phase acceptance and name the final approved commit.
2. Insert that exact 40-hex commit and its exact 40-hex Git tree into
   `P11_ACCEPTED_SOURCE`/`P11_ACCEPTED_TREE`. Preserve the ancestry guard.
3. Recheck the complete application-to-CI delta and the focused admission
   tests; enable only the P11 `false` workflow condition in the same reviewed
   change. Do not change the original branch condition or pin constants.
4. Root approves the final diff, then normally pushes the explicit branch.
   That approved push starts the build. There are no pull-request, tag,
   release or manual-dispatch entry points.
5. Retain the actual production archive/receipts promptly; Actions retention
   is seven days. Signing, notarization, exact-package installed preview and
   immutable new prerelease promotion are separate protected operations.
   Do not replace or amend RC1.

The runner remains restricted to trusted maintainers' code. GitHub mirror
pushes must not start duplicate application builds. This CI-only preparation
requires no new broad Rust/native application qualification.

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

The change affects dispatch, accepted-source admission, input provenance and
child-process isolation. Run the focused file:

```sh
python3 -m unittest discover -s tools/ci -p test_macos_builder.py -v
```

Tests use disposable local Git repositories and synthetic files to exercise
exact commit/tree plus CI-only modifications, wrong/missing acceptance pins,
unrelated same-tree source, provisional-ancestor rejection, immutable checkout
mismatch, product edits, dirty tracked/untracked inputs, empty deltas and
mode/symlink changes. The original RC1 additions-only, pinned-input drift and
cleared credential/override environment cases remain covered. No app is
compiled or opened, and no host credentials or network access are used.

The first approved CI run compiles the existing adapter and production app.
No Rust source is changed by this admission draft, so no Rust formatting/check
suite or native end-to-end rerun is needed. Original P11 and RC1 evidence
retains its real source/artifact/execution bindings.
