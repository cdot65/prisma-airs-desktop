#!/usr/bin/env python3
"""Combines a phase's per-chunk native acceptance records into the phase
record the `xtask native --phase` gate reads.

  merge_phase.py <phase>

Each member chunk's driver ran with AIRS_NATIVE_SUBJECT=<phase> against the
phase's prepared fixture bundle and wrote
docs/planning/evidence/<phase>/native-acceptance-<chunk>.json. This refuses
to merge unless every member's record exists and all name the same prepared
executable, bundle and fixture identity; checks are carried over verbatim.
"""

import json
import os
import sys
import time

ROOT = os.getcwd()


def main():
    phase = sys.argv[1]
    manifest = json.load(open(os.path.join(ROOT, "docs/architecture/checks.json")))
    members = manifest["phases"][phase]
    evidence = os.path.join(ROOT, "docs/planning/evidence", phase)
    prepared = json.load(open(os.path.join(evidence, "native-prepare.json")))
    records = []
    for chunk in members:
        path = os.path.join(evidence, f"native-acceptance-{chunk}.json")
        if not os.path.exists(path):
            raise SystemExit(f"{chunk}: {os.path.relpath(path, ROOT)} is missing")
        record = json.load(open(path))
        if record["app"]["executable_sha256"] != prepared["executable_sha256"] or record["app"]["bundle"] != prepared["bundle"]:
            raise SystemExit(f"{chunk}: its record was not made on the phase's prepared bundle")
        records.append((chunk, record))
    hosts = sorted({r["host"] for _, r in records})
    if len(hosts) != 1:
        raise SystemExit(f"member records come from different hosts: {hosts}")
    merged = {
        "schema": "airs-v2/native-acceptance/1",
        "subject": phase,
        "recorded_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "method": "cua",
        "performed_by": "; ".join(f"{chunk}: {r['performed_by']} (recorded {r['recorded_utc']})" for chunk, r in records),
        "host": hosts[0],
        "app": records[0][1]["app"],
        "checks": [check for _, r in records for check in r["checks"]],
        "limitations": [f"{chunk}: {item}" for chunk, r in records for item in r["limitations"]],
    }
    with open(os.path.join(evidence, "native-acceptance.json"), "w") as f:
        json.dump(merged, f, indent=2)
        f.write("\n")
    failed = [c["id"] for c in merged["checks"] if c["result"] != "pass"]
    print(f"{phase}: {len(merged['checks'])} checks from {members}; not passing: {failed}")
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
