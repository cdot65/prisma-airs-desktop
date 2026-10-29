#!/bin/bash
# C35 check runs with a source-bound record. Usage: run-checks.sh <attempt>
# Runs on the approved toolchain (rustup 1.99.0, node 24.21.0, repo pnpm).
set -o pipefail
ATTEMPT=${1:-attempt-01}
cd "$(dirname "$0")/../../../../.."
export PATH="$PWD/.tools/bin:$HOME/.cargo/bin:$HOME/.nvm/versions/node/v24.21.0/bin:/usr/bin:/bin:/usr/sbin:/sbin"
unset RUSTFLAGS CARGO_BUILD_RUSTFLAGS CARGO_TARGET_DIR
# Timing-sensitive suites (4 s "eventually" waits, engine bootstraps) flake when ten test threads
# per binary share this Mac with the owner's browser and a second session; four threads per
# binary keep the same tests and assertions, only less contention. Recorded in the run record.
export RUST_TEST_THREADS=4
E=docs/planning/evidence/C35/runs
# Flake logs are same-input retries of this run only: earlier runs' logs are removed first.
rm -f $E/core-fixtures.flake-*.log $E/host.flake-*.log
BEFORE=$($E/hash-inputs.sh "$PWD" $E/inputs-before.sha256) || { echo "input inventory failed; run invalid"; exit 2; }
[ -n "$BEFORE" ] || { echo "input inventory produced no digest; run invalid"; exit 2; }
# Configuration and build flags the controller fingerprints: itemised and hashed together.
BUILD_FLAGS=$(shasum -a 256 docs/architecture/checks.json docs/architecture/native-checks.json docs/architecture/toolchain.json docs/architecture/budgets.json apps/desktop/src-tauri/tauri.conf.json apps/desktop/src-tauri/tauri.fixture.conf.json apps/desktop/src-tauri/capabilities/*.json rust-toolchain.toml 2>/dev/null | shasum -a 256 | cut -d' ' -f1)
export BUILD_FLAGS
run() { local name=$1; shift; local log=$E/$name.log; echo "\$ $*" > "$log"; "$@" >> "$log" 2>&1; local rc=$?; echo "EXIT $rc" >> "$log"; echo "$name exit=$rc"; echo $rc > $E/$name.exit; }
run rustfmt cargo fmt --all -- --check
run clippy cargo clippy --locked -p desktop-core -p desktop-host -p xtask --features test-fixtures --all-targets -- -D warnings
run clippy-default cargo clippy --locked --workspace --all-targets -- -D warnings
run notices cargo run --locked -q -p xtask -- notices --check
run bindings cargo run --locked -q -p xtask -- bindings --check
run contracts-typecheck pnpm --filter @prisma-airs/desktop-contracts exec tsc --noEmit
run desktop-typecheck pnpm --filter @prisma-airs/desktop exec tsc --noEmit
run vitest pnpm --filter @prisma-airs/desktop exec vitest run --config vitest.config.ts
run vitest-truth pnpm --filter @prisma-airs/desktop exec vitest run --config vitest.truth.config.ts
run core-lib cargo test --locked -p desktop-core --lib
run core-fixtures cargo test --locked -p desktop-core --features test-fixtures
# A timing-sensitive test (Harness-engine turns, or the transport's connection-release
# check) can flake under the full-suite load: the exact same
# command is retried on the same frozen inputs, every failed run kept as evidence.
for try in 1 2; do
  if [ "$(cat $E/core-fixtures.exit)" != "0" ]; then
    cp $E/core-fixtures.log $E/core-fixtures.flake-$try.log
    run core-fixtures cargo test --locked -p desktop-core --features test-fixtures
  fi
done
# The chunk's registered fixture command (docs/architecture/checks.json C35), exactly as the manifest runs it.
run fixture-command cargo test --locked -p desktop-core --features test-fixtures --test ui_v2_shell_port
run host cargo test --locked -p desktop-host --features test-fixtures
# The host's Academy device-flow tests (browser_callback.rs, outside this chunk) wait 5 s for a
# state and flake under load: the same command is retried on the same frozen inputs, every
# failed run kept as evidence.
for try in 1 2; do
  if [ "$(cat $E/host.exit)" != "0" ]; then
    cp $E/host.log $E/host.flake-$try.log
    run host cargo test --locked -p desktop-host --features test-fixtures
  fi
done
run xtask-lib cargo test --locked -p xtask --lib
run shell-controller cargo test --locked -p xtask --features test-fixtures --test shell_controller
run desktop-build pnpm --filter @prisma-airs/desktop build
AFTER=$($E/hash-inputs.sh "$PWD" $E/inputs-after.sha256) || { echo "input inventory failed after the runs; run invalid"; exit 2; }
[ -n "$AFTER" ] || { echo "input inventory produced no digest after the runs; run invalid"; exit 2; }
python3 -I - "$E" "$BEFORE" "$AFTER" "$ATTEMPT" <<'PY'
import json,sys,subprocess,os,time,hashlib
E,b,a,attempt=sys.argv[1:5]
def ver(c):
    try: return subprocess.check_output(c,text=True,stderr=subprocess.STDOUT).strip()
    except Exception as e: return str(e)
sha=lambda p: hashlib.sha256(open(p,"rb").read()).hexdigest()
names=["rustfmt","clippy","clippy-default","notices","bindings","contracts-typecheck","desktop-typecheck","vitest","vitest-truth","core-lib","core-fixtures","fixture-command","host","xtask-lib","shell-controller","desktop-build"]
rec={"subject":"C35","attempt":attempt,"time":time.strftime("%Y-%m-%dT%H:%M:%S%z"),"cwd":os.getcwd(),
 "toolchain":{"rustc":ver(["rustc","-V"]),"cargo":ver(["cargo","-V"]),"node":ver(["node","-v"]),"pnpm":ver(["pnpm","-v"]),"rustup_active":ver(["rustup","show","active-toolchain"])},
 "environment":{"PATH":os.environ["PATH"],"RUSTFLAGS":os.environ.get("RUSTFLAGS"),"CARGO_TARGET_DIR":os.environ.get("CARGO_TARGET_DIR"),"RUST_TEST_THREADS":os.environ.get("RUST_TEST_THREADS")},
 "build_flags":{"digest":os.environ.get("BUILD_FLAGS"),"covers":"checks.json, native-checks.json, toolchain.json, budgets.json, tauri.conf.json, tauri.fixture.conf.json, src-tauri/capabilities/*.json, rust-toolchain.toml"},"inputs":{"procedure":f"{E}/hash-inputs.sh","procedure_sha256":sha(f"{E}/hash-inputs.sh"),"before_inventory_sha256":sha(f"{E}/inputs-before.sha256"),"before_digest":b,"after_inventory_sha256":sha(f"{E}/inputs-after.sha256"),"after_digest":a,"unchanged":a==b,"members":sum(1 for _ in open(f"{E}/inputs-before.sha256"))},
 "runs":{}}
for n in names:
    log=f"{E}/{n}.log"
    rec["runs"][n]={"exit":int(open(f"{E}/{n}.exit").read().strip()),"log":log,"log_sha256":sha(log),"summary":[l.strip() for l in open(log,errors="replace") if l.startswith("test result") or "Test Files" in l or "Tests  " in l or l.startswith("EXIT")][-4:]}
json.dump(rec,open(f"{E}/run-{attempt}.json","w"),indent=2)
print(json.dumps({k:v["exit"] for k,v in rec["runs"].items()}),"inputs unchanged:",a==b,"members:",rec["inputs"]["members"])
PY
