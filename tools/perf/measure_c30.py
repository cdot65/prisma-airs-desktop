#!/usr/bin/env python3
"""C30 measurement of the tenant-switch workload against the budget
registered before C30 measured it (docs/planning/evidence/C30/workload-budget.md,
registration in docs/planning/evidence/C30/workload-budget-registration.json).

  measure_c30.py --label calibration|acceptance

Run from the repository root on the prepared C30 fixture bundle. Only an
acceptance run writes docs/planning/evidence/<subject>/workload-perf.json
(in a phase, workload-perf-C30.json); it refuses to start unless the budget
table matches its registration. Raw samples are recorded; `xtask native`
recomputes the statistic.

Workload, on one launch in the v2 presentation against the synthetic
management endpoint on loopback, with Tenant A read once and Tenant B never
read:

- tenant-switch-home: from AXPress on the other tenant's row in the open
  switcher to the Home heading and the new tenant's name line existing in
  the accessibility tree (probe.swift activate, 2 ms polling); samples
  alternate A→B and B→A.
"""

import argparse
import hashlib
import json
import os
import sys
import tempfile
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "native-acceptance"))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C30")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
REGISTRATION = os.path.join(ROOT, "docs/planning/evidence/C30/workload-budget-registration.json")


def sha256_file(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def utc():
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def open_switcher(app):
    app.menu("Go", "Switch Tenant…")
    app.wait_for(lambda t: any(n.get("role") == "AXGroup" and (n.get("label") or "") == "Switch tenant" for n in t), 8)
    time.sleep(0.3)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", choices=["calibration", "acceptance"], required=True)
    label = parser.parse_args().label
    registration = json.load(open(REGISTRATION))
    table = os.path.join(ROOT, registration["budgets"])
    table_sha = sha256_file(table)
    if table_sha != registration["budgets_sha256"]:
        raise SystemExit("the workload budget table does not match its C30 registration")
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256_file(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c30-perf-")
    probe = os.path.join(work, "probe")
    built = fx.sh(["swiftc", "-O", os.path.join(HERE, "probe.swift"), "-o", probe])
    if built.returncode != 0:
        raise SystemExit(f"probe did not build: {built.stderr}")
    started = utc()
    app = fx.App(bundle, work, [])
    app.quit()
    fx.clean_fixture_root()
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), c8.SCAN_KEY)
    endpoint.install_roots()
    endpoint.start()
    mgmt = c8.MgmtEndpoint(os.path.join(work, "endpoint"), os.path.join(work, "mgmt"))
    mgmt.install()
    mgmt.start()
    mgmt.mode()
    metrics, details = {}, {}
    try:
        app.launch()
        # Two tenants in the legacy Settings, then the v2 presentation.
        app.open_settings()
        app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
        app.create_custom_tenant("Tenant A", endpoint.origin)
        app.save_key(c8.SCAN_KEY)
        c8.save_client(app)
        app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
        app.create_custom_tenant("Tenant B", endpoint.origin)
        app.make_active("Tenant A")
        app.ax("press", "AXCheckBox", "Use the new interface")
        app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Home") is not None, 15)
        # Tenant A read once: its Home shows the read status and items.
        app.ax("press", "AXButton", "Read now")
        app.wait_for(lambda t: any(x.startswith("Read finished at") for x in app.texts(t)), 30)
        details["a_read"] = True
        samples = []
        current = "Tenant A"
        for _ in range(12):
            other = "Tenant B" if current == "Tenant A" else "Tenant A"
            open_switcher(app)
            # Shown means the destination's Home with its own cached read status: Tenant A's
            # finished read, Tenant B's first read still pending (never the other's).
            status = "Read finished at" if other == "Tenant A" else "First read pending"
            out = fx.sh([probe, "activate", str(app.pid), "AXCheckBox", other, "Home", f"{other} ·", "10000", status])
            result = json.loads(out.stdout or "{}")
            if "ms" in result:
                samples.append(result["ms"])
            current = other
            time.sleep(0.5)
        metrics["tenant-switch-home"] = {"unit": "ms", "samples": samples[2:]}
        details["switches"] = len(samples)
    finally:
        app.quit()
        endpoint.stop()
        mgmt.stop()
        fx.clean_fixture_root()
    record = {
        "schema": "airs-v2-workload-perf/1",
        "label": label,
        "registration_sha256": table_sha,
        "started_utc": started,
        "finished_utc": utc(),
        "executable_sha256": prepared["executable_sha256"],
        "host": fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip(),
        "metrics": metrics,
        "details": details,
    }
    own = "workload-perf.json" if SUBJECT == "C30" else "workload-perf-C30.json"
    name = own if label == "acceptance" else f"workload-perf-calibration-{started.replace(':', '')}.json"
    path = os.path.join(EVIDENCE, name)
    with open(path, "w") as f:
        json.dump(record, f, indent=1)
        f.write("\n")
    for key, metric in metrics.items():
        ordered = sorted(metric["samples"])
        if ordered:
            p95 = ordered[max(1, -(-95 * len(ordered) // 100)) - 1]
            print(f"{key}: n={len(ordered)} p95={p95} max={ordered[-1]} {metric['unit']}")
    print(os.path.relpath(path, ROOT))
    return 0


if __name__ == "__main__":
    sys.exit(main())
