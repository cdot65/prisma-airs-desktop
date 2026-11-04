#!/usr/bin/env python3
"""Validate saved fixture ACL metadata only; never accesses any Keychain."""
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import sys

HERE = Path(__file__).resolve().parent
APP = "/Users/cdot/development/redtail/p09-queue/refinement-20261104/tooling/targets/core/aarch64-apple-darwin/release/bundle/macos/Prisma AIRS Desktop Fixture.app"
OLD = "3950c3d4b7e787d94930e3aac9bcd191cf70dd02"
NEW = "a579105e69b36d05fc8e5f66760688e60b8bb4c2"


def require(ok, message):
    if not ok:
        raise ValueError(message)


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main():
    require(len(sys.argv) == 1, "validator accepts no arguments")
    output = HERE / "validator-receipt.json"
    require(not output.exists(), "preserve previous validator result")
    saved = json.loads((HERE / "receipt.json").read_text())
    before = (HERE / "before.txt").read_bytes()
    after = (HERE / "after.txt").read_bytes()
    require(sha(HERE / "before.txt") == saved["before"]["sha256"], "before metadata drift")
    require(sha(HERE / "after.txt") == saved["after"]["sha256"], "after metadata drift")
    require(saved["account"] == "cr-bf6eae1f4e49ee61"
            and saved["retained_cdhash"] == OLD and saved["added_cdhash"] == NEW,
            "target metadata identity changed")
    app_line = f"            1: {APP} (status -2147415734)\n".encode()
    new_requirement = f'                requirement: cdhash H"{NEW}"\n'.encode()
    old_requirement = f'                requirement: cdhash H"{OLD}"\n'.encode()
    new_entry = app_line + new_requirement
    require(after.count(new_entry) == 1 and after.count(new_requirement) == 1,
            "new index1 path/CDHash must occur exactly once and together")
    require(before.count(old_requirement) == after.count(old_requirement) == 1,
            "old trusted app requirement must be retained exactly")
    require(after.count(old_requirement + new_entry) == 1, "new app was not appended after old app")
    group = after.split(b"    entry 1:\n")
    require(len(group) == 2, "ambiguous decrypt ACL")
    group = group[1].split(b"    entry 2:\n")[0]
    require(b"authorizations (6): decrypt derive export_clear export_wrapped mac sign\n" in group
            and group.count(b"        applications (2):\n") == 1
            and group.count(new_entry) == 1, "new app is not scoped to the expected decrypt ACL")
    count = b"        applications (2):\n"
    require(after.count(count) == 1, "ambiguous application-count edit")
    normalized = after.replace(new_entry, b"", 1).replace(count, b"        applications (1):\n", 1)
    require(normalized == before, "other item attributes, ACLs, applications or prompt selectors changed")
    original = HERE.parents[4] / saved["original_helper_receipt"]["path"]
    require(original.is_file() and sha(original) == saved["original_helper_receipt"]["sha256"],
            "original helper outcome was changed")
    result = {
        "schema": "airs-p11-saved-fixture-acl-validator/1", "verified": True,
        "recorded_utc": datetime.now(timezone.utc).isoformat(),
        "argv": [sys.executable, *sys.argv], "cwd": str(Path.cwd()),
        "validator_sha256": sha(Path(__file__)),
        "saved_root_receipt_sha256": sha(HERE / "receipt.json"),
        "before_sha256": sha(HERE / "before.txt"), "after_sha256": sha(HERE / "after.txt"),
        "normalized_after_sha256": hashlib.sha256(normalized).hexdigest(),
        "new_index1_exact_path_and_cdhash": True,
        "all_other_metadata_byte_identical": True,
        "original_helper_receipt_sha256": sha(original),
        "original_helper_outcome_preserved": True,
        "security_or_keychain_calls": False, "item_values_read": False
    }
    with output.open("x") as stream:
        json.dump(result, stream, indent=2)
        stream.write("\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
