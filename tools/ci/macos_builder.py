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
P11_REF = "refs/heads/infra/macos-candidate-p11-20261104"
# Ancestry evidence only; it is NOT an accepted P11 source pin.
P11_PROVISIONAL_ANCESTOR = "62580dec9287d05b7a4aae03ca0625247f28542c"
# Root inserts the final independently accepted commit/tree only after P11's
# phase gate, together with enabling this branch in macos-builder.yml.
P11_ACCEPTED_SOURCE = None
P11_ACCEPTED_TREE = None
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


def full_commit(value):
    return isinstance(value, str) and len(value) == 40 and all(c in "0123456789abcdef" for c in value)


def validate_context(context, head):
    require(context.get("GITHUB_REPOSITORY") == REPOSITORY, "unexpected repository")
    ref = context.get("GITHUB_REF")
    require(ref in (REF, P11_REF), "unexpected branch")
    require(context.get("GITHUB_EVENT_NAME") == "push", "only maintainer branch pushes are allowed")
    require(context.get("GITHUB_SHA") == head and len(head) == 40, "checkout differs from triggering commit")
    if ref == P11_REF:
        require(full_commit(P11_ACCEPTED_SOURCE) and full_commit(P11_ACCEPTED_TREE),
                "P11 lane is disabled until its final accepted source and tree are pinned after phase review")
        return P11_ACCEPTED_SOURCE
    return BASE


def validate_source(ref, actual_base, actual_tree):
    expected_base = P11_ACCEPTED_SOURCE if ref == P11_REF else BASE
    require(actual_base == expected_base, "accepted source did not resolve to its exact commit")
    if ref == P11_REF:
        require(actual_tree == P11_ACCEPTED_TREE, "accepted P11 tree differs from the reviewed pin")


def validate_delta(entries, ref=REF):
    require(bool(entries), "expected declared CI-only changes to accepted base")
    allowed = {"A", "M"} if ref == P11_REF else {"A"}
    for status, path in entries:
        require(status in allowed and path in CI_FILES, f"non-CI change from accepted base: {status} {path}")


def validate_p11_modes(raw, entries):
    modes = []
    for line in raw.splitlines():
        metadata, path = line.split("\t", 1)
        fields = metadata.split()
        require(len(fields) == 5 and fields[0].startswith(":"), "invalid CI mode delta")
        old_mode, new_mode, _, _, status = fields
        old_mode = old_mode[1:]
        require(path in CI_FILES and status in ("A", "M"), "unexpected P11 mode delta")
        require(new_mode in ("100644", "100755")
                and ((status == "A" and old_mode == "000000")
                     or (status == "M" and old_mode == new_mode)),
                f"P11 CI path changed type or mode: {path}")
        modes.append((status, path))
    require(sorted(modes) == sorted(tuple(row) for row in entries), "CI mode/path delta mismatch")


def admit_checkout(context, env, cwd=ROOT):
    # No input materialization, shims, caches, install or compiler subprocess
    # occurs before all source/admission checks below finish successfully.
    head = run(["git", "rev-parse", "HEAD"], env, True, cwd=cwd).strip()
    base = validate_context(context, head)
    actual_base = run(["git", "rev-parse", "--verify", base + "^{commit}"], env, True, cwd=cwd).strip()
    actual_tree = run(["git", "rev-parse", "--verify", base + "^{tree}"], env, True, cwd=cwd).strip()
    validate_source(context["GITHUB_REF"], actual_base, actual_tree)
    run(["git", "merge-base", "--is-ancestor", base, head], env, cwd=cwd)
    if context["GITHUB_REF"] == P11_REF:
        # Qualified source is ancestry only, never a substitute acceptance pin.
        run(["git", "merge-base", "--is-ancestor", P11_PROVISIONAL_ANCESTOR, base], env, cwd=cwd)
    changes = run(["git", "diff", "--name-status", "--no-renames", base, head], env, True, cwd=cwd)
    delta = [line.split("\t", 1) for line in changes.splitlines()]
    validate_delta(delta, context["GITHUB_REF"])
    if context["GITHUB_REF"] == P11_REF:
        raw = run(["git", "diff", "--raw", "--no-abbrev", "--no-renames", base, head], env, True, cwd=cwd)
        validate_p11_modes(raw, delta)
    require(not run(["git", "status", "--porcelain", "--untracked-files=all"], env, True, cwd=cwd).strip(),
            "checkout is not clean")
    return {"head": head, "base": actual_base, "tree": actual_tree, "delta": delta,
            "lane": "p11-accepted-source" if context["GITHUB_REF"] == P11_REF else "rc1-original"}


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
    admission = admit_checkout(os.environ, env)
    head, base, delta = admission["head"], admission["base"], admission["delta"]
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
        "repository": REPOSITORY, "build_commit": head, "accepted_application_base": base,
        "accepted_application_tree": admission["tree"], "admission_lane": admission["lane"],
        "provisional_qualification_ancestor": P11_PROVISIONAL_ANCESTOR if admission["lane"] == "p11-accepted-source" else None,
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
