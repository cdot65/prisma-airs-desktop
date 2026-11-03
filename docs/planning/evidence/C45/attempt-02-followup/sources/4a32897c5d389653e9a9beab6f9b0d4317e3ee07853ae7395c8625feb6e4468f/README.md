# Prisma AIRS Desktop v2

Successor to Prisma AIRS Desktop: React presentation in Tauri, a focused Rust
application core, the native Rust SCM SDK, and shared Academy presentation.
First target: macOS Apple Silicon, distributed as a signed and notarized DMG.
Implementation proceeds through ordered, independently reviewed chunks; see
[status](docs/planning/status.md) for what exists and what is verified.

For the current Worf VM transition, start with [WORF-START-HERE](docs/planning/WORF-START-HERE.md). It includes the exact source/worktree checkpoint verifier.

## Layout

```text
crates/desktop-core/          Rust application core: native authority, wire DTOs
apps/desktop/src-tauri/       desktop-host: Tauri window/origin admission, menus
apps/desktop/src/             React application composition
packages/desktop-contracts/   generated wire declarations, typed facade, fixtures
crates/xtask/                 build-time check/review/completion controller
tools/reference-validator/    one-off independent C01 bootstrap validator
```

## Building from a clean checkout

```text
corepack enable pnpm --install-directory .tools/bin      # pnpm 10.6.5 (packageManager)
cargo run --locked -p xtask -- fetch-inputs --source <path to ai-security-academy clone>
cargo run --locked -p xtask -- check --chunk C01 --manifest docs/architecture/checks.json
cargo run --locked -p xtask -- test  --chunk C01 --manifest docs/architecture/checks.json
```

The controller runs only the argv commands in
[checks.json](docs/architecture/checks.json) under the environment contract in
[toolchain.json](docs/architecture/toolchain.json); it never reads `.env` files.

Start with the [implementation plan](docs/planning/IMPLEMENTATION-PLAN.md),
[chunk instructions](docs/planning/chunks/README.md), [decisions](docs/planning/decisions.md),
the [design](docs/architecture/DESIGN.md) and [ADRs](docs/adr/0001-native-operation-authority.md).
Newly authored material uses [Apache-2.0](LICENSE); upstream terms remain intact.
Independent review: Codex reviews every chunk and phase (D026).

Owner-specified repository:
`ssh://git@git-ssh.cdot.io/cdot/prisma-airs-desktop-v2.git`.
