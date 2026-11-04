#!/bin/bash
# Deterministic inventory of the inputs P09's checks compile, test or build:
# the Rust crates and host, the desktop app sources and configuration, the
# contracts package sources and fixtures, the Academy shared source actually
# consumed (.work/academy/src and its installed packages, staged from
# .inputs/academy), the native acceptance tools, the lockfiles, the workspace
# and toolchain manifests, the harness pin and every docs/architecture/*.json.
# Generated Python bytecode caches are excluded; the captured pending-tree fixture
# consumed by the L19 driver regression is explicitly included.
# Every listed path must exist: a missing one fails the inventory (exit 2)
# instead of producing a partial digest.
# Usage: hash-inputs.sh <repo-root> <inventory-out>; prints the digest.
set -euo pipefail
cd "$1"
DIRS=(crates apps/desktop/src apps/desktop/src-tauri packages/desktop-contracts/src packages/desktop-contracts/fixtures tools/native-acceptance tools/parity tools/academy .work/academy/src .work/academy/packages .inputs/academy)
FILES=(docs/planning/evidence/C29/live/L19/attempt-01/wizard-verify.json Cargo.toml Cargo.lock rust-toolchain.toml pnpm-lock.yaml package.json pnpm-workspace.yaml resources/harness/pin.json apps/desktop/package.json apps/desktop/index.html packages/desktop-contracts/package.json packages/desktop-contracts/tsconfig.json)
for p in "${DIRS[@]}"; do [ -d "$p" ] || { echo "inventory: missing directory $p" >&2; exit 2; }; done
for p in "${FILES[@]}"; do [ -f "$p" ] || { echo "inventory: missing file $p" >&2; exit 2; }; done
{
  find "${DIRS[@]}" -type f -not -path '*/__pycache__/*' -not -path '*/target/*' -not -path '*/node_modules/*' -not -path '*/gen/schemas/*' -not -path '*/.git/*' -print0 | sort -z | xargs -0 shasum -a 256
  shasum -a 256 "${FILES[@]}" apps/desktop/*.ts apps/desktop/*.json docs/architecture/*.json
} | LC_ALL=C sort -k2 > "$2"
[ -s "$2" ] || { echo "inventory: empty" >&2; exit 2; }
shasum -a 256 "$2" | cut -d' ' -f1
