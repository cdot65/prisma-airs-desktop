#!/bin/bash
# Deterministic inventory of every input the C27 controller runs compile or
# read: every file under crates/xtask and crates/desktop-core (xtask's only
# local path dependency, including its assets/ and workbench/*.json read by
# include_str!), resources/harness/pin.json (compiled into desktop-core by
# include_str! in src/harness/engine.rs), the workspace Cargo.toml (source of
# the CARGO_PKG_VERSION env-dep), Cargo.lock, rust-toolchain.toml and every
# docs/architecture/*.json manifest. Coverage against the compiler's dep-info
# is verified separately by verify-coverage.py.
# Usage: hash-inputs.sh <repo-root> <inventory-out>
# Prints the digest: sha256 of the inventory file (sorted "sha256  path" lines).
set -euo pipefail
cd "$1"
{
  find crates/xtask crates/desktop-core -type f -not -path '*/target/*' -print0 | sort -z | xargs -0 shasum -a 256
  shasum -a 256 Cargo.toml Cargo.lock rust-toolchain.toml resources/harness/pin.json docs/architecture/*.json
} | LC_ALL=C sort -k2 > "$2"
shasum -a 256 "$2" | cut -d' ' -f1
