#!/usr/bin/env python3
"""Reseals the pinned native Harness engine for v2 (C19, D046).

  seal.py <v1 darwin-arm64.tar.gz> <repo>

The input is v1's sealed runtime archive at v1 commit 74754c02 (stored with
Git LFS as sha256 e79f564a...). Only the engine and its notices are taken:
bin/airs-harness, BUILD-INFO.json, DEPENDENCIES.json, LICENSE, NOTICE and
licenses/engine/. Node, the launcher scripts, the managed CLI and its
node_modules are left out: v2 ships no Node, Bun or TypeScript backend.

Every input is checked before anything is written: the archive's hash, the
binary's hash against BUILD-INFO and v1's runtime pin, and BUILD-INFO's
source commit against the engine pin. The output is deterministic (sorted
entries, fixed owner, mode and time, gzip without a timestamp), so the same
input always reseals to the same bytes:

  resources/harness/darwin-arm64.tar.gz   (Git LFS)
  resources/harness/pin.json              (the compiled pin; plain text)
"""

import gzip
import hashlib
import io
import json
import os
import sys
import tarfile
from pathlib import PurePosixPath

V1_COMMIT = "74754c02b19d87187b6dca81f64550a669b0f841"
V1_ARCHIVE_SHA256 = "e79f564a5210241488a49c4756ff061dff3d336e23709f5fb7adfd4bde0928aa"
ENGINE_COMMIT = "9331a5839bc6a763d94dee7584746f58bbbe1181"
BINARY_SHA256 = "f8e49cee8b0905818c9271e91b2ca8f39b67075e602e5db7db683f9d37aee50c"
RUNTIME_VERSION = "0.1.7-desktop.2"
PROTOCOL = "airs-desktop/1"
NOTICES = ("BUILD-INFO.json", "DEPENDENCIES.json", "LICENSE", "NOTICE")
MAX_MEMBER = 512 * 1024 * 1024
MAX_MEMBERS = 4096


def sha256(data):
    return hashlib.sha256(data).hexdigest()


def take(archive):
    """The engine and its notices from v1's archive, refusing anything that
    is not a plain, relative regular file."""
    wanted = {}
    with tarfile.open(archive, "r:gz") as source:
        for count, item in enumerate(source):
            if count >= 20000:
                raise SystemExit("v1 archive has too many members")
            path = PurePosixPath(item.name)
            if path.is_absolute() or ".." in path.parts or "\\" in item.name:
                raise SystemExit(f"unsafe path in v1 archive: {item.name!r}")
            name = path.as_posix().removeprefix("./")
            if item.isdir():
                continue
            if name == "bin/airs-harness":
                target = "airs-harness"
            elif name.startswith("licenses/") and name.removeprefix("licenses/") in NOTICES:
                target = name.removeprefix("licenses/")
            elif name.startswith("licenses/engine/"):
                target = name
            else:
                continue
            if not item.isfile() or item.size > MAX_MEMBER or target in wanted:
                raise SystemExit(f"unexpected member in v1 archive: {item.name!r}")
            wanted[target] = source.extractfile(item).read()
            if len(wanted) > MAX_MEMBERS:
                raise SystemExit("too many engine notices")
    return wanted


def main():
    archive, repo = sys.argv[1], sys.argv[2]
    with open(archive, "rb") as f:
        if sha256(f.read()) != V1_ARCHIVE_SHA256:
            raise SystemExit("the v1 archive is not the pinned one")
    files = take(archive)
    for name in ("airs-harness", *NOTICES):
        if name not in files:
            raise SystemExit(f"v1 archive lacks {name}")
    info = json.loads(files["BUILD-INFO.json"])
    if (sha256(files["airs-harness"]) != BINARY_SHA256 or info["binary_sha256"] != BINARY_SHA256
            or info["source_commit"] != ENGINE_COMMIT or info["target"] != "aarch64-apple-darwin"
            or info["version"] != RUNTIME_VERSION):
        raise SystemExit("the engine does not match its build record or the pin")
    inventory = {name: sha256(data) for name, data in sorted(files.items())}
    # One digest over the whole inventory: path, NUL, hash, newline each.
    resources = sha256("".join(f"{n}\0{h}\n" for n, h in inventory.items()).encode())
    pin = {
        "version": 1,
        "protocol": PROTOCOL,
        "runtimeVersion": RUNTIME_VERSION,
        "engineCommit": ENGINE_COMMIT,
        "executable": "airs-harness",
        "binarySha256": BINARY_SHA256,
        "resourcesSha256": resources,
    }
    manifest = json.dumps({"pin": pin, "files": inventory}, indent=1, sort_keys=True).encode() + b"\n"
    files["MANIFEST.json"] = manifest

    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w", format=tarfile.PAX_FORMAT) as out:
        for name in sorted(files):
            item = tarfile.TarInfo(f"harness/{name}")
            item.size = len(files[name])
            item.mode = 0o755 if name == "airs-harness" else 0o644
            item.mtime = 0
            item.uid = item.gid = 0
            item.uname = item.gname = ""
            out.addfile(item, io.BytesIO(files[name]))
    packed = io.BytesIO()
    with gzip.GzipFile(fileobj=packed, mode="wb", mtime=0, compresslevel=9, filename="") as gz:
        gz.write(raw.getvalue())
    data = packed.getvalue()

    out_dir = os.path.join(repo, "resources", "harness")
    os.makedirs(out_dir, exist_ok=True)
    with open(os.path.join(out_dir, "darwin-arm64.tar.gz"), "wb") as f:
        f.write(data)
    record = dict(pin, archive="resources/harness/darwin-arm64.tar.gz", archiveSha256=sha256(data),
                  archiveBytes=len(data), files=len(inventory) + 1,
                  source={"v1Commit": V1_COMMIT, "v1Archive": "packaging/harness/darwin-arm64.tar.gz",
                          "v1ArchiveSha256": V1_ARCHIVE_SHA256, "buildInfoSha256": inventory["BUILD-INFO.json"]})
    with open(os.path.join(out_dir, "pin.json"), "w") as f:
        f.write(json.dumps(record, indent=2, sort_keys=True) + "\n")
    print(json.dumps(record, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
