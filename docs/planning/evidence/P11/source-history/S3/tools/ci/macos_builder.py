#!/usr/bin/env python3
"""One trusted Apple Silicon compile; no signing, release gates or live calls."""
import hashlib
import json
import os
from pathlib import Path
import platform
import shutil
import subprocess
import sys
from datetime import datetime, timezone

BASE = "1241916e46156b6fe821f98b93bf281c255a7cbe"
REPOSITORY = "cdot/prisma-airs-desktop-v2"
REF = "refs/heads/infra/macos-builder-20261103"
ACADEMY_ORIGIN = "ssh://git@git-ssh.cdot.io/cdot.io/ai-security-academy.git"
ACADEMY_COMMIT = "532809aa4bb2e1484e11ca9f7a414328260c7901"
ACADEMY_TREE = "73e1c80889af89311b374808fce1f99386ced2fe95bc3a54429c40a72ce5530d"
HARNESS_SHA = "10729706be0bf2ec2bae1d21155fe866b047960e73881583397887c083fce173"
HARNESS_PATH = "resources/harness/darwin-arm64.tar.gz"
CI_FILES = {
    ".forgejo/workflows/macos-builder.yml",
    "crates/xtask/examples/ci_macos_build.rs",
    "tools/ci/macos_builder.py",
    "tools/ci/test_macos_builder.py",
    "docs/planning/ci/macos-builder.md",
}
PROFILE = Path("/Users/cdot/.local/share/airs-toolchains/profiles/airs-20261102/bin")
CACHE = Path("/Users/cdot/.local/share/airs-desktop-forgejo-runner/cache")
INPUTS = CACHE.parent / "inputs"
ROOT = Path(__file__).resolve().parents[2]


def require(condition, message):
    if not condition:
        raise RuntimeError(message)


def sha256(path):
    digest = hashlib.sha256()
    with path.open("rb") as source:
        for block in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def child_environment(source, transport=False):
    # Do not inherit action tokens, signing values, SSH agents or compiler flags.
    env = {k: source[k] for k in ("HOME", "USER", "LOGNAME", "TMPDIR", "LANG", "LC_ALL") if k in source}
    env.update({
        "PATH": f"{ROOT / '.tools/bin'}:{PROFILE}:{Path.home() / '.cargo/bin'}:/opt/homebrew/bin:/usr/bin:/bin:/usr/sbin:/sbin",
        "RUSTUP_TOOLCHAIN": "1.99.0", "CI": "1", "CARGO_TERM_COLOR": "never",
        "NO_COLOR": "1", "COREPACK_ENABLE_DOWNLOAD_PROMPT": "0", "COREPACK_ENABLE_AUTO_PIN": "0",
        "CARGO_HOME": str(CACHE / "cargo-public-build"),
        "XDG_CONFIG_HOME": str(ROOT / "target/ci-private-config"),
        "NPM_CONFIG_USERCONFIG": "/dev/null", "NPM_CONFIG_GLOBALCONFIG": "/dev/null",
        "GIT_TERMINAL_PROMPT": "0", "GIT_LFS_SKIP_SMUDGE": "1",
    })
    if transport:
        # Jadzia's existing read-only key authentication was verified without
        # an SSH agent; do not depend on an operator's forwarded session.
        env["GIT_SSH_COMMAND"] = "ssh -o BatchMode=yes -o IdentityAgent=none -o ConnectTimeout=10 -o ConnectionAttempts=1"
    return env


def validate_context(context, head):
    require(context.get("GITHUB_REPOSITORY") == REPOSITORY, "unexpected repository")
    require(context.get("GITHUB_REF") == REF, "unexpected branch")
    require(context.get("GITHUB_EVENT_NAME") == "push", "only maintainer branch pushes are allowed")
    require(context.get("GITHUB_SHA") == head and len(head) == 40, "checkout differs from triggering commit")


def validate_delta(entries):
    require(bool(entries), "expected declared CI-only additions to accepted base")
    for status, path in entries:
        require(status == "A" and path in CI_FILES, f"non-CI change from accepted base: {status} {path}")


def validate_pins(academy, harness):
    require(academy["origin"] == ACADEMY_ORIGIN and academy["commit"] == ACADEMY_COMMIT
            and academy["tree_sha256"] == ACADEMY_TREE, "Academy pin differs from reviewed build input")
    require(harness["archive"] == HARNESS_PATH and harness["archiveSha256"] == HARNESS_SHA
            and harness["archiveBytes"] == 113497192, "Harness pin differs from reviewed build input")


def run(argv, env, capture=False, cwd=ROOT):
    return subprocess.run(argv, cwd=cwd, env=env, check=True, text=True,
                          stdout=subprocess.PIPE if capture else None).stdout


def main():
    started = datetime.now(timezone.utc).isoformat()
    require(platform.system() == "Darwin" and platform.machine() == "arm64", "native Apple Silicon required")
    env = child_environment(os.environ)
    transport = child_environment(os.environ, transport=True)
    head = run(["git", "rev-parse", "HEAD"], env, True).strip()
    validate_context(os.environ, head)
    run(["git", "merge-base", "--is-ancestor", BASE, head], env)
    changes = run(["git", "diff", "--name-status", "--no-renames", BASE, head], env, True)
    delta = [line.split("\t", 1) for line in changes.splitlines()]
    validate_delta(delta)
    require(not run(["git", "status", "--porcelain", "--untracked-files=all"], env, True).strip(), "checkout is not clean")
    academy = json.loads((ROOT / "docs/architecture/inputs.lock.json").read_text())["academy"]
    harness = json.loads((ROOT / "resources/harness/pin.json").read_text())
    validate_pins(academy, harness)
    for name in ("node", "pnpm"):
        target = PROFILE / name
        require(target.is_file(), f"verified project tool missing: {target}")
        link = ROOT / ".tools/bin" / name
        link.parent.mkdir(parents=True, exist_ok=True)
        require(not link.exists() and not link.is_symlink(), f"unexpected preexisting tool shim: {link}")
        link.symlink_to(target)
    for path in (CACHE / "cargo-public-build", CACHE / "pnpm-10.6.5", INPUTS, ROOT / "target/ci-private-config"):
        path.mkdir(parents=True, exist_ok=True)
    run(["rustc", "--version"], env)
    run(["node", "--version"], env)
    run(["pnpm", "--version"], env)
    # Only one LFS object is materialized; historical gate binaries stay pointers.
    archive = ROOT / HARNESS_PATH
    cached_harness = INPUTS / HARNESS_SHA / "darwin-arm64.tar.gz"
    if cached_harness.is_file():
        require(sha256(cached_harness) == HARNESS_SHA, "cached Harness checksum mismatch")
        shutil.copy2(cached_harness, archive)
    else:
        require(shutil.which("git-lfs", path=transport["PATH"]),
                "preprovision the exact Harness input cache; Git LFS is unavailable on this host")
        run(["git", "-c", "remote.origin.url=ssh://git@git-ssh.cdot.io/cdot/prisma-airs-desktop-v2.git",
             "lfs", "pull", "--include=" + HARNESS_PATH, "--exclude="], transport)
    require(sha256(archive) == HARNESS_SHA and archive.stat().st_size == 113497192, "Harness archive checksum/size mismatch")
    snapshot = INPUTS / ACADEMY_TREE / "academy"
    xtask = ["cargo", "run", "--locked", "-p", "xtask", "--"]
    if snapshot.is_dir():
        shutil.copytree(snapshot, ROOT / ".inputs/academy", symlinks=True)
        academy_materialization = "preprovisioned immutable snapshot; verified by shared-prepare"
    else:
        source = INPUTS / "academy.git"
        if not source.exists():
            run(["git", "init", "--bare", str(source)], transport)
        # Fetch the exact reviewed commit, never the changing branch head.
        run(["git", "-C", str(source), "fetch", "--no-tags", "--depth=1", ACADEMY_ORIGIN, ACADEMY_COMMIT], transport)
        run(xtask + ["fetch-inputs", "--source", str(source)], env)
        academy_materialization = "exact-commit read-only SSH fetch + verified git archive"
    run(xtask + ["shared-prepare", "--input", ".inputs/academy", "--scratch", ".work/academy"], env)
    run(["pnpm", "install", "--frozen-lockfile", "--ignore-scripts", "--store-dir", str(CACHE / "pnpm-10.6.5")], env)
    run(["cargo", "run", "--locked", "-p", "xtask", "--example", "ci_macos_build"], env)
    output = ROOT / "target/ci-macos-build"
    build = json.loads((output / "build.json").read_text())
    app = ROOT / build["app"]
    arch = run(["/usr/bin/lipo", "-archs", str(app / "Contents/MacOS/prisma-airs-desktop")], env, True).strip()
    require(arch == "arm64", "unexpected application executable architecture")
    packaged = output / "Prisma-AIRS-Desktop-1.0.0-macos-arm64-internal-unsigned.tar.gz"
    # Preserve the bundle and inherited engine signature without opening it.
    run(["/usr/bin/tar", "-czf", str(packaged), "-C", str(app.parent), app.name], env)
    receipt = {
        "schema": "airs-ci-macos-provenance/1", "kind": "internal-unsigned-build-only",
        "repository": REPOSITORY, "build_commit": head, "accepted_application_base": BASE,
        "ci_only_delta": {path: sha256(ROOT / path) for _, path in delta},
        "run_id": os.environ["GITHUB_RUN_ID"], "run_attempt": os.environ.get("GITHUB_RUN_ATTEMPT", "1"),
        "started_utc": started, "finished_utc": datetime.now(timezone.utc).isoformat(),
        "target": "aarch64-apple-darwin", "architecture": arch,
        "input_hashes": {name: sha256(ROOT / name) for name in (
            "Cargo.lock", "pnpm-lock.yaml", "rust-toolchain.toml", "docs/architecture/toolchain.json",
            "docs/architecture/inputs.lock.json", "resources/harness/pin.json")},
        "academy": {"commit": ACADEMY_COMMIT, "tree_sha256": ACADEMY_TREE, "materialization": academy_materialization},
        "harness_archive_sha256": sha256(archive), "archive": packaged.name,
        "archive_sha256": sha256(packaged), "build_receipt_sha256": sha256(output / "build.json"),
        "xcode": run(["/usr/bin/xcodebuild", "-version"], env, True).strip(),
        "macos": run(["/usr/bin/sw_vers", "-productVersion"], env, True).strip(),
        "qualification": "not executed; C26/P08 remains open; no signing/notarization/install/live/native tests/publication",
    }
    (output / "provenance.json").write_text(json.dumps(receipt, sort_keys=True, indent=2) + "\n")
    (output / "SHA256SUMS").write_text("".join(f"{sha256(p)}  {p.name}\n" for p in sorted(output.iterdir()) if p.name != "SHA256SUMS"))
    print(json.dumps({"build_commit": head, "archive_sha256": receipt["archive_sha256"], "qualification": "build-only"}))


if __name__ == "__main__":
    try:
        main()
    except (RuntimeError, subprocess.CalledProcessError) as error:
        print(f"Mac builder stopped: {error}", file=sys.stderr)
        sys.exit(1)
