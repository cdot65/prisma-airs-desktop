#!/usr/bin/env python3
"""C11 measurement of the large management collections (lists, filters)
workload against the budgets registered before C11 implemented it
(docs/planning/evidence/C11/workload-budget.md, registration in
docs/planning/evidence/C11/workload-budget-registration.json).

  measure_c11.py --label calibration|acceptance

Run from the repository root on the prepared C11 fixture bundle. Only an
acceptance run writes docs/planning/evidence/<subject>/workload-perf.json,
and it refuses to start unless the budget table matches its registration.
Raw samples are recorded; `xtask native` recomputes every statistic.

Workloads, on one launch with a Keychain management client against the
synthetic management endpoint (mgmt_server.py) serving a 24-hour inventory
of 2,500 sessions, which answers immediately and logs each request:

- sessions-next-page: from AXPress on Next page to the next page's range
  text ("Sessions 26–50 of 2500 · read at …", rendered in the same commit as
  its 25 rows) existing in the Accessibility tree (tools/perf/probe.swift
  activate, 2 ms polling); 20 consecutive pages.
- detector-search-1000: a detector search that matches one session, so it
  walks the bounded 1,000 sessions (ten pages of 100); from AXPress on
  Search to its coverage statement existing in the tree. The previous
  result is cleared before each sample.
- sessions-process-tree-resident: after reading ten pages and opening ten
  sessions with a transaction each, the app plus its WebKit processes' RSS
  sampled once per second for 20 s.
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
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C11")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
REGISTRATION = os.path.join(ROOT, "docs/planning/evidence/C11/workload-budget-registration.json")
HEADING = "Investigate sessions"
SESSIONS = 2500


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


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", choices=["calibration", "acceptance"], required=True)
    label = parser.parse_args().label
    registration = json.load(open(REGISTRATION))
    table = os.path.join(ROOT, registration["budgets"])
    table_sha = sha256_file(table)
    if table_sha != registration["budgets_sha256"]:
        raise SystemExit("the workload budget table does not match its C11 registration")
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256_file(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c11-perf-")
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
    # One session in the window reports the detector: the search walks all
    # of its bounded 1,000 sessions.
    mgmt.mode(sessionCount=SESSIONS, detectorEvery=10 ** 9)
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
        app.ax("press", "AXButton", "Sessions &")
        _, ok = c8.text(app, f"Sessions 1–25 of {SESSIONS}", 20)
        if not ok:
            raise RuntimeError("the sessions inventory did not load")
        time.sleep(1)

        # ---- sessions-next-page ----
        timings, runs = [], []
        for page in range(1, 21):
            first = page * 25 + 1
            data = probe_activate(probe, app, "AXButton", "Next page", f"Sessions {first}–{first + 24} of {SESSIONS}")
            timings.append(data.get("ms"))
            runs.append({"page": page + 1, **data})
            time.sleep(0.5)
        if any(t is None for t in timings):
            raise RuntimeError(f"a page did not appear in time: {runs}")
        metrics["sessions-next-page"] = {"unit": "ms", "samples": timings}
        details["sessions_next_page"] = runs
        app.ax("press", "AXButton", "First page")
        c8.text(app, f"Sessions 1–25 of {SESSIONS}", 15)

        # ---- detector-search-1000 ----
        app.type_into("AXTextField", "Search by session ID, scan ID or detector", "prompt_injection")
        statement = "1 session reporting prompt_injection among 1,000 sessions read from the last 24 hours"
        timings, runs = [], []
        for n in range(10):
            if fx.App.node(app.tree(), "AXButton", "Clear search") is not None:
                app.ax("press", "AXButton", "Clear search")
                app.wait_for(lambda t: not app.has_text(t, statement), 5)
            mark = len(mgmt.requests())
            data = probe_activate(probe, app, "AXButton", "Search", statement)
            walked = [r for r in mgmt.requests()[mark:] if r["path"].endswith("/sessionsoverview")]
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "pages_walked": len(walked), **data})
            time.sleep(0.5)
        if any(t is None for t in timings) or any(r["pages_walked"] != 10 for r in runs):
            raise RuntimeError(f"a detector search did not complete its bounded walk: {runs}")
        metrics["detector-search-1000"] = {"unit": "ms", "samples": timings}
        details["detector_search"] = runs
        app.ax("press", "AXButton", "Clear search")

        # ---- sessions-process-tree-resident ----
        for page in range(10):
            app.ax("press", "AXButton", "Next page")
            c8.text(app, f"Sessions {(page + 1) * 25 + 1}–", 15)
        opened = 0
        tree = app.tree()
        for node in [n for n in tree if n.get("role") == "AXButton" and (n.get("label") or "").startswith("Open session ")][:10]:
            session = node["label"].removeprefix("Open session ")
            app.ax("press", "AXButton", node["label"])
            c8.text(app, "3 scans", 12)
            app.ax("press", "AXButton", f"Details for scan scn-{session}-0")
            c8.text(app, '"verdict": "malicious"', 12)
            opened += 1
        if opened != 10:
            raise RuntimeError(f"opened {opened} sessions, not 10")
        resident = []
        for _ in range(20):
            resident.append(round(rss_mib(pids), 1))
            time.sleep(1)
        metrics["sessions-process-tree-resident"] = {"unit": "MiB", "samples": resident}
        details["requests"] = len(mgmt.requests())
        details["sessions_opened"] = opened
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
    # Recorded for another subject (a phase, or C24's cumulative closure)
    # under this chunk's name.
    own = "workload-perf.json" if SUBJECT == "C11" else "workload-perf-C11.json"
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
