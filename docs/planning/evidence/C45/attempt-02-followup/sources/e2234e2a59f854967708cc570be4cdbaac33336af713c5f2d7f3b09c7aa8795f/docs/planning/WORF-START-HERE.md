# Continue the current desktop work in the Worf VM

This is the 2026-11-01 continuation guide. It supersedes the old sibling
`p09-queue/handoff` instructions about Geordi, rsync and the older C44 worktree.
The target is the existing macOS VM on **Worf**. Kirk's local Tart VM `airs`
was only a bootstrap experiment; it produced no passing P09 native acceptance.
Its Python permission prompt is not a prerequisite for this Git migration.

## Get the current source

From an empty development directory in the Worf VM:

```sh
git clone --branch main ssh://git@git-ssh.cdot.io/cdot/prisma-airs-desktop-v2.git
cd prisma-airs-desktop-v2
git lfs install --local
git lfs pull
python3 -I tools/p09/verify-migration.py
git rev-parse HEAD origin/main
```

For an existing clean clone, use `git fetch origin`, `git switch main`,
`git pull --ff-only origin main`, then `git lfs pull` and the same verification.
Preserve local modifications before changing an existing checkout.
The verification checks the latest application/tooling source, all 221 previously
pending main files, every retained worktree file mapping, required commit ancestry
and the historical L19 executable. It intentionally describes this migration
checkpoint; later intentional implementation changes will change its hashes.

All chunk branch tips are retained as remote branches. C37/C39/C39b/C40/C41/
C42/C43/C44 were already merged. The C29 live and C44 scope commits were applied
as equivalent cherry-picks. The older `p09-merge` history and detached dry merge
are preserved as refs rather than replacing the newer integrated source.
The audit and exact file mapping are in
[worktree-preservation/2026-11-01](worktree-preservation/2026-11-01/README.md).
No original worktree was deleted or reset.

## Prepare build inputs

Use macOS Apple Silicon with a logged-in GUI account and Xcode Command Line
Tools/Swift. The contracted toolchain is Rust **1.99.0** (rustfmt, clippy,
`aarch64-apple-darwin`), Node **24.21.0** in its NVM directory, pnpm **10.6.5**,
Python 3, and Git LFS. Install those in the VM before running the following:

```sh
mkdir -p .tools/bin
corepack enable pnpm --install-directory .tools/bin
export PATH="$PWD/.tools/bin:$HOME/.cargo/bin:$HOME/.nvm/versions/node/v24.21.0/bin:/usr/bin:/bin:/usr/sbin:/sbin"
cargo fetch --locked
git clone --branch v2/desktop-presentation ssh://git@git-ssh.cdot.io/cdot.io/ai-security-academy.git ../ai-security-academy
cargo run --locked -p xtask -- fetch-inputs --source ../ai-security-academy
cargo run --locked -p xtask -- shared-prepare --input .inputs/academy --scratch .work/academy
pnpm install --frozen-lockfile --ignore-scripts
pnpm --dir .work/academy install --frozen-lockfile --ignore-scripts
cargo run --locked -p xtask -- shared-lifecycle --allowlist docs/architecture/shared-lifecycle.json --scratch .work/academy
pnpm --dir .work/academy exec playwright install chromium --only-shell
```

`fetch-inputs` uses the locked Academy commit **532809aa4bb2e1484e11ca9f7a414328260c7901**,
not the evolving branch tip. Keep the input lock unchanged for the move.
The Rust SCM SDK is the locked crates.io dependency; the Harness is the pinned
repository LFS archive. Neither needs an external source worktree. Recreate
build/dependency directories here; do not restore Kirk's absolute symlinks.

Use **Terminal.app** as the guest GUI execution context. Resolve its actual
Accessibility/System Events permission prompts in the VM. A 1920×1200 display
is suitable for the native matrix. Configure the Codex CLI/model in the VM for
fresh read-only reviews; host authentication files are not part of this transfer.
The owner's planning skill is preserved at `tools/p09/skills/grill-me/SKILL.md`
and can be installed under the guest's `~/.codex/skills/grill-me/`.

## Exact current standing and next work

- **C44 attempt-05: 9/9/9/9/9**, no required fixes/blockers/gaps; merged. Its
  opaque collection scope, render-time stale-editor retirement, packet isolation
  and complete legacy-map guard are in current source.
- **L19: actual Vulture wizard verification passed** on attempt-03 after the
  separately approved local credential correction. The original two reads and
  separately approved third are charged, with **no remaining live allowance**.
  Do not repeat the live probe. Redacted receipts and the exact historical
  executable (LFS) are retained; raw build paths/receipts/verdicts are unchanged.
- **C29 attempt-07: 9/9/9/8/8**, no blockers, accepted under D034 with R1/R2/E1/E2.
  Commit `03e572be` restores Gateway/key setup, rejected/fixed/stale verification,
  keyed Cancel/Keychain cleanup, relaunch/import/confinement checks and the full
  **24-behavior, 48-row** wizard matrix. Syntax and completeness-map checks pass.
  The new native flow has **not** passed yet.
- **P09 check attempt-02: all 18 checks passed** on unchanged inputs (541 frontend
  tests, 15 live-scope tests, 57 synthetic live-run tests, four L19 driver tests).
  Those records are Kirk history. The current app/build inputs match those tested
  bytes, including the owner work now committed for this move.
- C30/C31/C32 still need their predecessor-dependent re-reviews after C29 reaches
  full 9. The P09 phase review follows full native/check/member completion.

Archive the retained Kirk P09 preparation/incomplete native generation in a new
history directory before creating the Worf generation. Keep all verdict folders
immutable and choose fresh check/review attempt IDs. Then run:

```sh
cargo run --locked -p xtask -- native-prepare --phase P09 --target aarch64-apple-darwin --features test-fixtures
python3 -I docs/planning/evidence/P09/native-controller/run-p09-native.py c36
python3 -I docs/planning/evidence/P09/native-controller/run-p09-native.py c43
```

C36 proves the guest driver context; C43 proves the restored C29 native coverage.
Continue the other required drivers serially using the retained native controller
plan and assertion requirements. Every record must bind the newly prepared
executable. C44's cumulative driver runs last. Historical C24-N06 and C31-N01/N04
aggregates remain superseded as the map states; their old raw failures are not
rewritten or counted as passing checks.

Use a fresh P09 check attempt (next currently `attempt-03`) and the portable
helpers in [tools/p09](../../tools/p09/README.md) for C29 attempt-08, then C30,
C31 and C32 in order. Report every actual score and apply D034's 8-then-9 rule.
The copied phase packet builder is explicitly a disabled draft; finish its
current-P09 native/predecessor verification before using it for the phase review.
C26 still requires the documented manual review fallback; do not manufacture
controller completion.

After the P09 phase passes at 9, execute the owner-approved
[C45/P10 brief](C45-draft.md): Prisma AIRS Desktop, version 1.0.0,
`io.cdot.airs-desktop` / `.fixture`, fresh setup with no alpha/v2 migration,
legacy/variant/import cleanup, chunk 8→9 and phase 9. C45 has not started.
The preserved pending 2.0.0 version/release work is superseded at that later
chunk, not silently renamed during migration. October C26 installed/signing
records and `docs/release/2.0.0.md` are historical/draft; pushing source neither
publishes a release nor clears their remaining gates.

Private live manifests, Keychains, CLI tenant TOMLs, signing keys and authentication
profiles stay outside Git. They are not needed for the next fixture gates.
