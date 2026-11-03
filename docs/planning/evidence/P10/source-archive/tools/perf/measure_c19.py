#!/usr/bin/env python3
"""C19 measurement of the Harness native-engine startup and memory workload
against the budgets registered before C19 implemented it
(docs/planning/evidence/C19/workload-budget.md, registration in
docs/planning/evidence/C19/workload-budget-registration.json).

  measure_c19.py --label calibration|acceptance

Run from the repository root on the prepared C19 fixture bundle. Only an
acceptance run writes docs/planning/evidence/<subject>/workload-perf.json
(in a phase, workload-perf-C19.json), and it refuses to start unless the
budget table matches its registration. Raw samples are recorded; `xtask
native` recomputes every statistic.

Workloads, on one launch against the synthetic inference server
(tools/native-acceptance/mgmt_server.py) on loopback, with the workspace
Team A chosen and its inference key stored. Each Harness answer is
numbered by the server ("Answer <n>: Hello from the synthetic Gateway."),
so a sample waits for its own answer, never an earlier one.

- harness-start-ready: from AXPress on "Start conversation" to the
  conversation heading and "Ready." existing in the Accessibility tree
  (tools/perf/probe.swift activate, 2 ms polling). Each sample launches a
  new engine; between samples the conversation is ended and cleared. The
  first sample is the bundle's first engine launch.
- harness-first-output: short answer; from AXPress on "Send" to the
  answer's first text existing.
- harness-turn-interrupt: paced answer, interrupted about 1 s in; from
  AXPress on "Stop answer" to "The answer was stopped.".
- harness-end-reaped: from AXPress on "End conversation" to "You ended this
  conversation." existing, plus however long after that the engine's
  processes still existed (process table polled every 10 ms).
- harness-responsive-during-turn: while a paced answer streams, from
  AXPress on Settings to its heading and its first text existing; each
  sample is checked to have ended while the answer still streamed.
- harness-process-tree-resident: the app, its WebKit processes and the
  engine's processes, RSS sampled once per second for 20 s spanning two
  conversations: three short answers and one paced answer, interrupted.
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
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C19")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
REGISTRATION = os.path.join(ROOT, "docs/planning/evidence/C19/workload-budget-registration.json")
KEY = "SYNTHETIC-C19-PERF-KEY"
HEADING = "Conversation in Team A"
ENGINE = "Fixture.app/Contents/Resources/harness/airs-harness"


def sha256_file(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def utc():
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def webkit_pids():
    return set(int(p) for p in fx.sh(["pgrep", "-f", "com.apple.WebKit"]).stdout.split())


def engines():
    return [int(p) for p in fx.sh(["pgrep", "-f", ENGINE]).stdout.split()]


def rss_mib(pids):
    pids = [p for p in pids if p]
    out = fx.sh(["ps", "-o", "rss=", "-p", ",".join(str(p) for p in pids)]).stdout.split()
    return sum(int(v) for v in out) / 1024


def probe_activate(probe, app, role, label, heading, text, timeout=60000):
    out = fx.sh([probe, "activate", str(app.pid), role, label, heading, text, str(timeout)])
    return json.loads(out.stdout or "{}")


def answered(mgmt):
    """Inference requests the server has numbered so far."""
    return sum(1 for r in mgmt.requests() if r["path"] in ("/rt/v1/responses", "/rt/v1/chat/completions")
               and not r.get("closedByClient"))


def until_no_engine(timeout=10):
    """Milliseconds until no engine process is left."""
    start = time.monotonic()
    while engines():
        if time.monotonic() - start > timeout:
            return None
        time.sleep(0.01)
    return round((time.monotonic() - start) * 1000, 3)


def end_and_clear(app):
    if fx.App.node(app.tree(), "AXButton", "End conversation"):
        app.ax("press", "AXButton", "End conversation")
        app.wait_for(lambda t: app.has_text(t, "You ended this conversation."), 10)
    until_no_engine()
    if fx.App.node(app.tree(), "AXButton", "Clear conversation"):
        app.ax("press", "AXButton", "Clear conversation")
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Start conversation") is not None, 10)


def start(app):
    app.ax("press", "AXButton", "Start conversation")
    _, ok = app.wait_for(lambda t: app.has_text(t, "Ready."), 30)
    return ok


def type_prompt(app, text):
    app.type_into("AXTextArea", "Message", text)
    app.wait_for(lambda t: (fx.App.node(t, "AXButton", "Send") or {}).get("enabled") is not False, 5)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", choices=["calibration", "acceptance"], required=True)
    label = parser.parse_args().label
    registration = json.load(open(REGISTRATION))
    table = os.path.join(ROOT, registration["budgets"])
    table_sha = sha256_file(table)
    if table_sha != registration["budgets_sha256"]:
        raise SystemExit("the workload budget table does not match its C19 registration")
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256_file(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c19-perf-")
    probe = os.path.join(work, "probe")
    built = fx.sh(["swiftc", "-O", os.path.join(HERE, "probe.swift"), "-o", probe])
    if built.returncode != 0:
        raise SystemExit(f"probe did not build: {built.stderr}")
    started = utc()
    app = fx.App(bundle, work, [])
    app.quit()
    fx.sh(["pkill", "-9", "-f", ENGINE])
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
        app.ax("press", "AXButton", "Harness")
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Start conversation") is not None, 10)
        time.sleep(1)

        # ---- harness-start-ready ----
        timings, runs = [], []
        for n in range(10):
            end_and_clear(app)
            data = probe_activate(probe, app, "AXButton", "Start conversation", HEADING, "Ready.", 30000)
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "engines": len(engines()), **data})
        if any(t is None for t in timings) or not all(r["engines"] >= 1 for r in runs):
            raise RuntimeError(f"a conversation did not start: {runs}")
        metrics["harness-start-ready"] = {"unit": "ms", "samples": timings}
        details["start"] = runs

        # ---- harness-first-output ----
        timings, runs = [], []
        for n in range(10):
            app.wait_for(lambda t: app.has_text(t, "Answered.") or app.has_text(t, "Ready."), 10)
            serial = answered(mgmt) + 1
            type_prompt(app, f"Short answer {n + 1}, please.")
            data = probe_activate(probe, app, "AXButton", "Send", HEADING, f"Answer {serial}:", 30000)
            _, done = app.wait_for(lambda t: app.has_text(t, "Answered."), 15)
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "serial": serial, "answered": done, **data})
        if any(t is None for t in timings) or not all(r["answered"] for r in runs):
            raise RuntimeError(f"an answer did not arrive: {runs}")
        metrics["harness-first-output"] = {"unit": "ms", "samples": timings}
        details["first_output"] = runs

        # ---- harness-turn-interrupt ----
        mgmt.mode(harness="paced")
        timings, runs = [], []
        for n in range(10):
            type_prompt(app, f"A long answer {n + 1}.")
            app.ax("press", "AXButton", "Send")
            app.wait_for(lambda t: fx.App.node(t, "AXButton", "Stop answer") is not None
                         and app.has_text(t, "thirty-two bytes"), 15)
            time.sleep(1)
            data = probe_activate(probe, app, "AXButton", "Stop answer", HEADING, "The answer was stopped.", 10000)
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, **data})
            # The next sample's "stopped" must be its own: wait for Ready
            # state to be reached through another turn's status.
            app.wait_for(lambda t: fx.App.node(t, "AXButton", "Stop answer") is None, 10)
        if any(t is None for t in timings):
            raise RuntimeError(f"an answer was not stopped: {runs}")
        metrics["harness-turn-interrupt"] = {"unit": "ms", "samples": timings}
        details["interrupt"] = runs

        # ---- harness-responsive-during-turn ----
        timings, runs = [], []
        for n in range(20):
            app.ax("press", "AXButton", "Harness")
            app.wait_for(lambda t: fx.App.node(t, "AXTextArea", "Message") is not None, 10)
            type_prompt(app, f"Keep talking {n + 1}.")
            app.ax("press", "AXButton", "Send")
            app.wait_for(lambda t: fx.App.node(t, "AXButton", "Stop answer") is not None, 15)
            time.sleep(0.3)
            data = probe_activate(probe, app, "AXButton", "Settings", "Settings", "A desktop-wide preference.", 10000)
            app.ax("press", "AXButton", "Harness")
            _, running = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Stop answer") is not None, 10)
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "during_turn": running, **data})
            if running:
                app.ax("press", "AXButton", "Stop answer")
                app.wait_for(lambda t: fx.App.node(t, "AXButton", "Stop answer") is None, 10)
        if any(t is None for t in timings) or not all(r["during_turn"] for r in runs):
            raise RuntimeError(f"a destination switch was not measured during an answer: {runs}")
        metrics["harness-responsive-during-turn"] = {"unit": "ms", "samples": timings}
        details["responsive"] = runs
        mgmt.mode()

        # ---- harness-end-reaped ----
        timings, runs = [], []
        for n in range(10):
            if n:
                end_and_clear(app)
                if not start(app):
                    raise RuntimeError("a conversation did not start before ending it")
            data = probe_activate(probe, app, "AXButton", "End conversation", HEADING, "You ended this conversation.", 10000)
            after = until_no_engine()
            total = None if data.get("ms") is None or after is None else round(data["ms"] + after, 3)
            timings.append(total)
            runs.append({"sample": n + 1, "after_ms": after, **data})
        if any(t is None for t in timings):
            raise RuntimeError(f"a conversation did not end and reap: {runs}")
        metrics["harness-end-reaped"] = {"unit": "ms", "samples": timings}
        details["end"] = runs

        # ---- harness-process-tree-resident ----
        end_and_clear(app)
        resident = []
        stop = threading.Event()

        def sample():
            for _ in range(20):
                resident.append(round(rss_mib(pids + engines()), 1))
                if stop.wait(1):
                    break

        sampler = threading.Thread(target=sample)
        sampler.start()
        steps = [("start", start(app))]
        for n in range(2):
            type_prompt(app, f"Resident {n + 1}.")
            app.ax("press", "AXButton", "Send")
            _, ok = app.wait_for(lambda t: fx.App.node(t, "AXButton", "Stop answer") is None and app.has_text(t, "Answered."), 15)
            steps.append(("short", ok))
        end_and_clear(app)
        steps.append(("start", start(app)))
        type_prompt(app, "Resident 3.")
        app.ax("press", "AXButton", "Send")
        _, ok = app.wait_for(lambda t: app.has_text(t, "Answered."), 15)
        steps.append(("short", ok))
        mgmt.mode(harness="paced")
        type_prompt(app, "Resident long.")
        app.ax("press", "AXButton", "Send")
        app.wait_for(lambda t: app.has_text(t, "thirty-two bytes"), 15)
        time.sleep(1)
        app.ax("press", "AXButton", "Stop answer")
        _, ok = app.wait_for(lambda t: app.has_text(t, "The answer was stopped."), 10)
        steps.append(("paced-stopped", ok))
        sampler.join()
        mgmt.mode()
        end_and_clear(app)
        if len(resident) != 20 or not all(ok for _, ok in steps):
            raise RuntimeError(f"the resident workload did not complete: {steps}, {len(resident)} samples")
        metrics["harness-process-tree-resident"] = {"unit": "MiB", "samples": resident}
        details["resident_steps"] = steps
        details["harness_requests"] = sum(1 for r in mgmt.requests() if r.get("harness"))
    finally:
        app.quit()
        fx.sh(["pkill", "-9", "-f", ENGINE])
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
    own = "workload-perf.json" if SUBJECT == "C19" else "workload-perf-C19.json"
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
