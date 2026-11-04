#!/usr/bin/env python3
"""C18 measurement of the Gateway streaming and charts workload against the
budgets registered before C18 implemented it
(docs/planning/evidence/C18/workload-budget.md, registration in
docs/planning/evidence/C18/workload-budget-registration.json).

  measure_c18.py --label calibration|acceptance

Run from the repository root on the prepared C18 fixture bundle. Only an
acceptance run writes docs/planning/evidence/<subject>/workload-perf.json
(in a phase, workload-perf-C18.json), and it refuses to start unless the
budget table matches its registration. Raw samples are recorded; `xtask
native` recomputes every statistic.

Workloads, on one launch against the synthetic management and inference
server (tools/native-acceptance/mgmt_server.py) on loopback, with the
workspace Team A chosen and its inference key stored:

- gateway-stream-first-output: bulk stream (64 KiB in 2,048 server-sent
  chunks of 32 bytes); from AXPress on "Run prompt" to the Response heading
  and the first output text existing in the Accessibility tree
  (tools/perf/probe.swift activate, 2 ms polling). Each sample starts with
  no response shown.
- gateway-stream-64kib-complete: the same, to "Completed." (the output is
  then checked to hold all 64 KiB).
- gateway-stream-stop: paced stream (one chunk every 10 ms), stopped about
  1 s in; from AXPress on "Stop run" to "Stopped.".
- gateway-responsive-during-stream: while a paced stream runs, from AXPress
  on the Usage tab to its heading and its "Last 7 days" choice existing; each
  sample is checked to have ended while the stream still ran.
- gateway-usage-charts-30d: 720 hourly points per series and 200 model
  rows; from AXPress on "Refresh usage" (30 days) to the new requests total
  and the first model row existing. Samples alternate 720 and 719 points so
  each refresh shows a total the previous one did not.
- gateway-process-tree-resident: the app plus its WebKit processes' RSS
  sampled once per second for 20 s spanning three bulk streams, one paced
  stream stopped mid-way and two 30-day chart reads.
"""

import argparse
import hashlib
import json
import os
import sys
import tempfile
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "native-acceptance"))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c16 as c16  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C18")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
REGISTRATION = os.path.join(ROOT, "docs/planning/evidence/C18/workload-budget-registration.json")
KEY = "SYNTHETIC-C18-PERF-KEY"
OUTPUT = 64 * 1024


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


def probe_activate(probe, app, role, label, heading, text, timeout=60000):
    out = fx.sh([probe, "activate", str(app.pid), role, label, heading, text, str(timeout)])
    return json.loads(out.stdout or "{}")


def clear(app):
    if fx.App.node(app.tree(), "AXButton", "Clear response"):
        app.ax("press", "AXButton", "Clear response")
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Clear response") is None, 10)


def output_length(app):
    tree = app.tree()
    texts = [t for t in app.texts(tree) if t.startswith("C18-STREAM-")]
    return max((len(t) for t in texts), default=0)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", choices=["calibration", "acceptance"], required=True)
    label = parser.parse_args().label
    registration = json.load(open(REGISTRATION))
    table = os.path.join(ROOT, registration["budgets"])
    table_sha = sha256_file(table)
    if table_sha != registration["budgets_sha256"]:
        raise SystemExit("the workload budget table does not match its C18 registration")
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256_file(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c18-perf-")
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
    mgmt.mode(chat="bulk")
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
        c16.open_gateway(app)
        c8.text(app, "2 workspaces", 15)
        app.ax("press", "AXRadioButton", "Team A")
        c8.text(app, "2 providers · complete", 15)
        c16.tab(app, "Playground")
        c8.text(app, "No inference key is stored", 12)
        app.type_into("AXTextField", "Runtime endpoint", f"{mgmt.origin}/rt/v1")
        app.type_into("AXTextField", "Workspace API key with completions access", KEY, secure=True)
        app.ax("press", "AXButton", "Store key for Team A")
        c8.text(app, "A key for Team A is stored", 15)
        app.type_into("AXTextField", "Model", "@openai-prod/gpt-4o")
        app.type_into("AXTextArea", "Prompt", "Stream 64 KiB.")
        time.sleep(1)

        # ---- gateway-stream-first-output and gateway-stream-64kib-complete ----
        first, complete, runs = [], [], []
        for n in range(10):
            clear(app)
            data = probe_activate(probe, app, "AXButton", "Run prompt", "Response", "C18-STREAM-", 30000)
            first.append(data.get("ms"))
            _, done = app.wait_for(lambda t: app.has_text(t, "Completed."), 30)
            runs.append({"sample": n + 1, **data, "completed": done})
        for n in range(10):
            clear(app)
            data = probe_activate(probe, app, "AXButton", "Run prompt", "Response", "Completed.", 30000)
            length = output_length(app)
            complete.append(data.get("ms"))
            runs.append({"sample": n + 11, **data, "output": length})
        if any(t is None for t in first + complete) or any(r.get("output", OUTPUT) != OUTPUT for r in runs):
            raise RuntimeError(f"a bulk stream did not complete in full: {runs}")
        metrics["gateway-stream-first-output"] = {"unit": "ms", "samples": first}
        metrics["gateway-stream-64kib-complete"] = {"unit": "ms", "samples": complete}
        details["bulk"] = runs

        # ---- gateway-stream-stop ----
        mgmt.mode(chat="paced")
        timings, runs = [], []
        for n in range(10):
            clear(app)
            app.ax("press", "AXButton", "Run prompt")
            app.wait_for(lambda t: app.has_text(t, "C18-STREAM-"), 15)
            time.sleep(1)
            data = probe_activate(probe, app, "AXButton", "Stop run", "Response", "Stopped.", 10000)
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, **data})
        if any(t is None for t in timings):
            raise RuntimeError(f"a stream was not stopped: {runs}")
        metrics["gateway-stream-stop"] = {"unit": "ms", "samples": timings}
        details["stop"] = runs

        # ---- gateway-responsive-during-stream ----
        timings, runs = [], []
        for n in range(20):
            c16.tab(app, "Playground")
            app.wait_for(lambda t: fx.App.node(t, "AXButton", "Run prompt") is not None, 10)
            clear(app)
            app.ax("press", "AXButton", "Run prompt")
            app.wait_for(lambda t: app.has_text(t, "C18-STREAM-"), 15)
            time.sleep(0.3)
            data = probe_activate(probe, app, "AXRadioButton", "Usage", "Usage of Team A", "Last 7 days", 10000)
            c16.tab(app, "Playground")
            _, running = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Stop run") is not None, 10)
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "during_stream": running, **data})
            if running:
                app.ax("press", "AXButton", "Stop run")
                app.wait_for(lambda t: app.has_text(t, "Stopped."), 10)
        if any(t is None for t in timings) or not all(r["during_stream"] for r in runs):
            raise RuntimeError(f"a tab switch was not measured during a stream: {runs}")
        metrics["gateway-responsive-during-stream"] = {"unit": "ms", "samples": timings}
        details["responsive"] = runs

        # ---- gateway-usage-charts-30d ----
        c16.tab(app, "Usage")
        app.wait_for(lambda t: app.has_text(t, "Total: "), 15)
        app.ax("press", "AXRadioButton", "Last 30 days")
        app.wait_for(lambda t: app.has_text(t, "Last 30 days as the service reported it."), 15)
        timings, runs = [], []
        for n in range(10):
            points = 720 - (n % 2)
            mgmt.mode(chat="paced", gwChartPoints=points, gwModelRows=200)
            expected = f"Total: {5 * points:,}"
            data = probe_activate(probe, app, "AXButton", "Refresh usage", "Usage of Team A", expected, 30000)
            _, model = app.wait_for(lambda t: app.has_text(t, "model-000"), 10)
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "points": points, "model": model, **data})
        if any(t is None for t in timings) or not all(r["model"] for r in runs):
            raise RuntimeError(f"a 30-day read was not shown: {runs}")
        metrics["gateway-usage-charts-30d"] = {"unit": "ms", "samples": timings}
        details["charts"] = runs

        # ---- gateway-process-tree-resident ----
        resident = []
        stop = threading.Event()

        def sample():
            for _ in range(20):
                resident.append(round(rss_mib(pids), 1))
                if stop.wait(1):
                    break

        sampler = threading.Thread(target=sample)
        sampler.start()
        steps = []
        c16.tab(app, "Playground")
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Run prompt") is not None, 10)
        mgmt.mode(chat="bulk", gwChartPoints=720, gwModelRows=200)
        for _ in range(3):
            clear(app)
            app.ax("press", "AXButton", "Run prompt")
            _, ok = app.wait_for(lambda t: app.has_text(t, "Completed."), 30)
            steps.append(("bulk", ok))
        mgmt.mode(chat="paced", gwChartPoints=720, gwModelRows=200)
        clear(app)
        app.ax("press", "AXButton", "Run prompt")
        app.wait_for(lambda t: app.has_text(t, "C18-STREAM-"), 15)
        time.sleep(1)
        app.ax("press", "AXButton", "Stop run")
        _, ok = app.wait_for(lambda t: app.has_text(t, "Stopped."), 10)
        steps.append(("paced-stopped", ok))
        c16.tab(app, "Usage")
        for _ in range(2):
            app.ax("press", "AXButton", "Refresh usage")
            _, ok = app.wait_for(lambda t: app.has_text(t, "model-000"), 30)
            steps.append(("charts", ok))
        sampler.join()
        if len(resident) != 20 or not all(ok for _, ok in steps):
            raise RuntimeError(f"the resident workload did not complete: {steps}, {len(resident)} samples")
        metrics["gateway-process-tree-resident"] = {"unit": "MiB", "samples": resident}
        details["resident_steps"] = steps
        details["inference_requests"] = sum(1 for r in mgmt.requests() if r["path"] == "/rt/v1/chat/completions")
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
    own = "workload-perf.json" if SUBJECT == "C18" else "workload-perf-C18.json"
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
