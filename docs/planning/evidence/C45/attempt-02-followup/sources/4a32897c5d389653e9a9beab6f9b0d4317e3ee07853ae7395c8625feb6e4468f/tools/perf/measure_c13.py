#!/usr/bin/env python3
"""C13 measurement of the large red-team/model collections workload against
the budgets registered before C13 implemented it
(docs/planning/evidence/C13/workload-budget.md, registration in
docs/planning/evidence/C13/workload-budget-registration.json).

  measure_c13.py --label calibration|acceptance

Run from the repository root on the prepared C13 fixture bundle. Only an
acceptance run writes docs/planning/evidence/<subject>/workload-perf.json
(in a phase, workload-perf-C13.json),
and it refuses to start unless the budget table matches its registration.
Raw samples are recorded; `xtask native` recomputes every statistic.

Workloads, on one launch with a Keychain management client against the
synthetic product endpoints (mgmt_server.py: 100 Red Teaming scans with
30-day statistics, 100 Model Security scans, AgentGuard 30-day statistics
and 100 skill scans, a 1,000-attack report), which answer immediately:

- redteam-scans-open: from AXPress on the Red Team & Models destination
  (after leaving it, so the view starts with nothing held) to its heading
  and the scan list's coverage line ("Scans · 100 read · complete", shown
  with the statistics, tiles and 100 rows from one result) existing in the
  Accessibility tree (tools/perf/probe.swift activate, 2 ms polling).
- redteam-attacks-1000: from AXPress on a finished scan's attack results to
  the completeness statement after all ten pages of 100 were read.
- model-summary-open: from AXPress on the Models & skills tab (first shown
  in a fresh view) to the skill scans' coverage line, rendered from the one
  result that carries every source's state and counts.
- redteam-model-process-tree-resident: after opening both tabs ten times
  each and reading one 1,000-attack report, the app plus its WebKit
  processes' RSS sampled once per second for 20 s.
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
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C13")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
REGISTRATION = os.path.join(ROOT, "docs/planning/evidence/C13/workload-budget-registration.json")
HEADING = "Red Teaming and model observation"
DESTINATION = "Red Team & Models"
SCANS_READ = "Scans · 100 read · complete"
SKILLS_READ = "Skill scans · 100 read · complete"
ATTACKS_READ = "250 of 1000 succeeded · read 100 per page · all attacks read"


def sha256_file(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def utc():
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def webkit_pids():
    return set(int(p) for p in fx.sh(["pgrep", "-f", "com.apple.WebKit"]).stdout.split())


def rss_mib(pids):
    out = fx.sh(["ps", "-o", "rss=", "-p", ",".join(str(p) for p in pids)]).stdout.split()
    return sum(int(v) for v in out) / 1024


def probe_activate(probe, app, role, label, text):
    out = fx.sh([probe, "activate", str(app.pid), role, label, HEADING, text, "30000"])
    return json.loads(out.stdout or "{}")


def role_of(app, label):
    node = next((n for n in app.tree() if n.get("label") == label and n.get("role") in ("AXButton", "AXRadioButton", "AXTab")), None)
    if node is None:
        raise RuntimeError(f"no control labelled {label!r}")
    return node["role"]


def leave(app):
    """Leaves the destination so the next opening starts with nothing held."""
    app.open_settings()
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", HEADING) is None, 8)
    time.sleep(0.4)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", choices=["calibration", "acceptance"], required=True)
    label = parser.parse_args().label
    registration = json.load(open(REGISTRATION))
    table = os.path.join(ROOT, registration["budgets"])
    table_sha = sha256_file(table)
    if table_sha != registration["budgets_sha256"]:
        raise SystemExit("the workload budget table does not match its C13 registration")
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256_file(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c13-perf-")
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
    mgmt.mode(attackTotal=1000)
    metrics, details = {}, {}
    try:
        before = webkit_pids()
        app.launch()
        time.sleep(2)
        pids = [app.pid, *sorted(webkit_pids() - before)]
        details["process_tree"] = {"app": app.pid, "webkit": pids[1:]}
        app.open_settings()
        app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
        app.create_custom_tenant("Tenant A", endpoint.origin)
        app.save_key(c8.SCAN_KEY)
        c8.save_client(app)

        # ---- redteam-scans-open ----
        timings, runs = [], []
        for n in range(20):
            leave(app)
            mark = len(mgmt.requests())
            data = probe_activate(probe, app, "AXButton", DESTINATION, SCANS_READ)
            reads = [r for r in mgmt.requests()[mark:] if r["path"].startswith("/ai-red-teaming/")]
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "requests": len(reads), **data})
            time.sleep(0.4)
        if any(t is None for t in timings):
            raise RuntimeError(f"Red Teaming did not open in time: {runs}")
        metrics["redteam-scans-open"] = {"unit": "ms", "samples": timings}
        details["redteam_scans_open"] = runs

        # ---- redteam-attacks-1000 ----
        opener = next(n["label"] for n in app.tree() if n.get("role") == "AXButton"
                      and (n.get("label") or "").startswith("Open attack results for scan "))
        timings, runs = [], []
        for n in range(10):
            if fx.App.node(app.tree(), "AXButton", "Close attack results") is not None:
                app.ax("press", "AXButton", "Close attack results")
                app.wait_for(lambda t: not app.has_text(t, ATTACKS_READ), 5)
            mark = len(mgmt.requests())
            data = probe_activate(probe, app, "AXButton", opener, ATTACKS_READ)
            pages = [r for r in mgmt.requests()[mark:] if r["path"].endswith("/list-attacks")]
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "pages_read": len(pages), **data})
            time.sleep(0.5)
        if any(t is None for t in timings) or any(r["pages_read"] != 10 for r in runs):
            raise RuntimeError(f"an attack report was not read in full: {runs}")
        metrics["redteam-attacks-1000"] = {"unit": "ms", "samples": timings}
        details["redteam_attacks"] = runs

        # ---- model-summary-open ----
        timings, runs = [], []
        for n in range(20):
            leave(app)
            app.ax("press", "AXButton", DESTINATION)
            c8.text(app, SCANS_READ, 15)
            mark = len(mgmt.requests())
            data = probe_activate(probe, app, role_of(app, "Models & skills"), "Models & skills", SKILLS_READ)
            reads = [r for r in mgmt.requests()[mark:] if r["path"].startswith(("/aims/", "/aiag/"))]
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "requests": len(reads), **data})
            time.sleep(0.4)
        if any(t is None for t in timings) or any(r["requests"] != 3 for r in runs):
            raise RuntimeError(f"Models & skills did not open with its three sources: {runs}")
        metrics["model-summary-open"] = {"unit": "ms", "samples": timings}
        details["model_summary_open"] = runs

        # ---- redteam-model-process-tree-resident ----
        leave(app)
        app.ax("press", "AXButton", DESTINATION)
        c8.text(app, SCANS_READ, 15)
        app.ax("press", "AXButton", opener)
        _, read = c8.text(app, ATTACKS_READ, 30)
        if not read:
            raise RuntimeError("the 1,000-attack report was not read")
        resident = []
        for _ in range(20):
            resident.append(round(rss_mib(pids), 1))
            time.sleep(1)
        metrics["redteam-model-process-tree-resident"] = {"unit": "MiB", "samples": resident}
        details["requests"] = len(mgmt.requests())
        details["non_reads"] = [r["path"] for r in mgmt.requests() if r["method"] != "GET" and r["path"] != "/oauth2/access_token"]
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
    # A phase may measure several workloads: each keeps its own record.
    own = "workload-perf.json" if SUBJECT == "C13" else "workload-perf-C13.json"
    name = own if label == "acceptance" else f"workload-perf-calibration-{started.replace(':', '')}.json"
    path = os.path.join(EVIDENCE, name)
    with open(path, "w") as f:
        json.dump(record, f, indent=1)
        f.write("\n")
    for key, metric in metrics.items():
        ordered = sorted(metric["samples"])
        p95 = ordered[max(1, -(-95 * len(ordered) // 100)) - 1]
        print(f"{key}: n={len(ordered)} p95={p95} max={ordered[-1]} {metric['unit']}")
    print(os.path.relpath(path, ROOT))
    return 0


if __name__ == "__main__":
    sys.exit(main())
