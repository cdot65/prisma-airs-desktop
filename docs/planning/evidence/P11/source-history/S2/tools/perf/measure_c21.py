#!/usr/bin/env python3
"""C21 measurement of the Harness tool workload against the budgets
registered before C21 was built into the app
(docs/planning/evidence/C21/workload-budget.md, registration in
docs/planning/evidence/C21/workload-budget-registration.json).

  measure_c21.py --label calibration|acceptance

Run from the repository root on the prepared C21 fixture bundle. Only an
acceptance run writes docs/planning/evidence/<subject>/workload-perf.json
(in a phase, workload-perf-C21.json), and it refuses to start unless the
budget table matches its registration. Raw samples are recorded; `xtask
native` recomputes every statistic.

Every timing is tools/perf/probe.swift `activate`: from AXPress on the
named control to the named heading and a static text with the named prefix
existing in the accessibility tree (2 ms polling). The synthetic Gateway
plays the model (mode "harnessTool"): a turn's first request is answered
with the tool call, the request carrying its output with the numbered text
"Answer <n>: ...", so a sample waits for its own answer.

- harness-tool-read-answer: the model reads the topics inventory; from Send
  to the numbered answer.
- harness-approval-shown: the model runs `ls` in a read-only project of 20
  files; from Send to "Run a command" under "Waiting for your approval".
- harness-approved-command-done: the same; from Allow to the numbered answer.
- harness-proposal-staged: the model proposes a topic update; from Send to
  its summary under "Proposed changes" (five per conversation).
- harness-proposal-review-ready: from Review to Administration's
  "Proposed by the Harness: ..." heading and the review's impact text; each
  review is discarded and the Harness opened again before the next.
- harness-tools-process-tree-resident: RSS of the app, its WebKit processes
  and the engine, once a second for 20 s spanning three reads, one approved
  command and one proposal reviewed and discarded.
"""

import argparse
import hashlib
import json
import os
import secrets
import shutil
import sys
import tempfile
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "native-acceptance"))
import fixture_app as fx  # noqa: E402
import acceptance_c08 as c8  # noqa: E402
import acceptance_c09 as c9  # noqa: E402
import acceptance_c16 as c16  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C21")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
REGISTRATION = os.path.join(ROOT, "docs/planning/evidence/C21/workload-budget-registration.json")
KEY = "SYNTHETIC-C21-PERF-KEY"
HEADING = "Conversation in Team A"
ENGINE = "Fixture.app/Contents/Resources/harness/airs-harness"
TOPIC = "11111111-1111-4111-8111-111111111111"
IMPACT = "Applies only to this tenant"


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


def requests(mgmt):
    """Gateway requests so far (each one numbers the server's next answer)."""
    return sum(1 for r in mgmt.requests() if r["path"] in ("/rt/v1/responses", "/rt/v1/chat/completions")
               and not r.get("closedByClient"))


def tool(mgmt, name, arguments, namespace="airs_desktop"):
    spec = {"name": name, "arguments": json.dumps(arguments)}
    if namespace:
        spec["namespace"] = namespace
    mgmt.mode(harnessTool=spec)


def read(mgmt):
    tool(mgmt, "inventory", {"resource": "topics", "limit": 3})


def listing(mgmt):
    tool(mgmt, "exec_command", {"cmd": "ls", "yield_time_ms": 1000, "login": False}, namespace=None)


def proposal(mgmt, n):
    tool(mgmt, "proposeChange", {"kind": "topics", "action": "update", "target": TOPIC,
                                 "body": {"topic_name": "Finance", "description": f"Money talk {n}", "examples": [], "revision": 1},
                                 "summary": f"Clarify Finance {n}"})


def type_prompt(app, text):
    app.type_into("AXTextArea", "Message", text)
    app.wait_for(lambda t: (fx.App.node(t, "AXButton", "Send") or {}).get("enabled") is not False, 5)


def settled(app, serial, timeout=30):
    return app.wait_for(lambda t: app.has_text(t, f"Answer {serial}:") and app.has_text(t, "Answered.")
                        and fx.App.node(t, "AXButton", "Decline") is None, timeout)[1]


def end_and_clear(app):
    if fx.App.node(app.tree(), "AXButton", "End conversation"):
        app.ax("press", "AXButton", "End conversation")
        app.wait_for(lambda t: app.has_text(t, "You ended this conversation."), 10)
    deadline = time.time() + 10
    while engines() and time.time() < deadline:
        time.sleep(0.05)
    if fx.App.node(app.tree(), "AXButton", "Clear conversation"):
        app.ax("press", "AXButton", "Clear conversation")
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Start conversation") is not None, 10)


def start(app):
    app.ax("press", "AXButton", "Start conversation")
    _, ok = app.wait_for(lambda t: app.has_text(t, "Ready."), 30)
    return ok


def open_harness(app):
    app.ax("press", "AXButton", "Harness")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Harness") is not None, 10)


def discard_review(app):
    """Discards the open change review in Administration (its Close) and
    returns to the Harness."""
    c9.press_exact(app, "Close")
    app.wait_for(lambda t: fx.App.node(t, "AXHeading", "Proposed by the Harness") is None, 10)
    open_harness(app)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", choices=["calibration", "acceptance"], required=True)
    label = parser.parse_args().label
    registration = json.load(open(REGISTRATION))
    table = os.path.join(ROOT, registration["budgets"])
    table_sha = sha256_file(table)
    if table_sha != registration["budgets_sha256"]:
        raise SystemExit("the workload budget table does not match its C21 registration")
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256_file(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c21-perf-")
    # The engine refuses system temporary folders as a project's home.
    project = os.path.join(ROOT, "target", f"perf-c21-{secrets.token_hex(6)}")
    os.makedirs(project)
    for n in range(20):
        with open(os.path.join(project, f"file-{n:02d}.txt"), "w") as f:
            f.write(f"content {n}\n")
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
        app.launch()
        app.open_settings()
        app.type_into("AXTextField", "TSG ID (optional)", c8.TSG)
        app.create_custom_tenant("Tenant A", endpoint.origin)
        app.save_key(c8.SCAN_KEY)
        c8.save_client(app)
        c16.open_gateway(app)
        c8.text(app, "2 workspaces", 25)
        app.ax("press", "AXRadioButton", "Team A")
        c8.text(app, "2 providers · complete", 15)
        c16.tab(app, "Playground")
        app.type_into("AXTextField", "Runtime endpoint", f"{mgmt.origin}/rt/v1")
        app.type_into("AXTextField", "Workspace API key with completions access", KEY, secure=True)
        app.ax("press", "AXButton", "Store key for Team A")
        c8.text(app, "A key for Team A is stored", 15)
        # The project, staged for the fixture's folder sheet.
        with open(os.path.join(fx.DATA, "fixture-project-folder"), "w") as f:
            f.write(project)
        open_harness(app)
        app.ax("press", "AXButton", "Choose a folder…")
        _, chosen = app.wait_for(lambda t: app.has_text(t, project), 10)
        if not chosen or not start(app):
            raise RuntimeError("the project conversation did not start")

        # ---- harness-tool-read-answer ----
        read(mgmt)
        timings, runs = [], []
        for n in range(10):
            serial = requests(mgmt) + 2
            type_prompt(app, f"Read topics {n + 1}")
            data = probe_activate(probe, app, "AXButton", "Send", HEADING, f"Answer {serial}:", 30000)
            ok = settled(app, serial)
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "answered": ok, **data})
        if any(t is None for t in timings) or not all(r["answered"] for r in runs):
            raise RuntimeError(f"a read was not answered: {runs}")
        metrics["harness-tool-read-answer"] = {"unit": "ms", "samples": timings}
        details["read"] = runs

        # ---- harness-approval-shown and harness-approved-command-done ----
        listing(mgmt)
        shown, done, runs = [], [], []
        for n in range(10):
            serial = requests(mgmt) + 2
            type_prompt(app, f"List the folder {n + 1}")
            first = probe_activate(probe, app, "AXButton", "Send", "Waiting for your approval", "Run a command", 30000)
            second = probe_activate(probe, app, "AXButton", "Allow", HEADING, f"Answer {serial}:", 30000)
            ok = settled(app, serial)
            shown.append(first.get("ms"))
            done.append(second.get("ms"))
            runs.append({"sample": n + 1, "answered": ok, "shown": first, "done": second})
        if any(t is None for t in shown + done) or not all(r["answered"] for r in runs):
            raise RuntimeError(f"a command was not approved and answered: {runs}")
        metrics["harness-approval-shown"] = {"unit": "ms", "samples": shown}
        metrics["harness-approved-command-done"] = {"unit": "ms", "samples": done}
        details["command"] = runs

        # ---- harness-proposal-staged and harness-proposal-review-ready ----
        staged, ready, runs = [], [], []
        for n in range(10):
            if n % 5 == 0:
                # At most eight proposals per conversation.
                end_and_clear(app)
                if not start(app):
                    raise RuntimeError("a proposal conversation did not start")
            proposal(mgmt, n + 1)
            serial = requests(mgmt) + 2
            type_prompt(app, f"Propose {n + 1}")
            first = probe_activate(probe, app, "AXButton", "Send", "Proposed changes", f"Clarify Finance {n + 1}", 30000)
            ok = settled(app, serial)
            heading = f"Proposed by the Harness: update custom topic {TOPIC}"
            # This proposal's own Review: the newest listed is the last one.
            reviews = [x for x in app.tree() if x.get("role") == "AXButton" and x.get("label") == "Review"]
            second = probe_activate(probe, app, "AXButton", "Review", heading, IMPACT, 30000) if len(reviews) == 1 else {}
            staged.append(first.get("ms"))
            ready.append(second.get("ms"))
            runs.append({"sample": n + 1, "answered": ok, "staged": first, "review": second, "reviews_listed": len(reviews)})
            discard_review(app)
        if any(t is None for t in staged + ready) or not all(r["answered"] for r in runs):
            raise RuntimeError(f"a proposal was not staged and reviewed: {runs}")
        metrics["harness-proposal-staged"] = {"unit": "ms", "samples": staged}
        metrics["harness-proposal-review-ready"] = {"unit": "ms", "samples": ready}
        details["proposal"] = runs
        end_and_clear(app)

        # ---- harness-tools-process-tree-resident ----
        app.quit()
        before = webkit_pids()
        app.launch()
        time.sleep(2)
        pids = [app.pid, *sorted(webkit_pids() - before)]
        details["process_tree"] = {"app": app.pid, "webkit": pids[1:]}
        c16.open_gateway(app)
        c8.text(app, "2 workspaces", 25)
        app.ax("press", "AXRadioButton", "Team A")
        c8.text(app, "2 providers · complete", 15)
        open_harness(app)
        if not start(app):
            raise RuntimeError("the resident conversation did not start")
        resident = []
        stop = threading.Event()

        def sample():
            for _ in range(20):
                resident.append(round(rss_mib(pids + engines()), 1))
                if stop.wait(1):
                    break

        sampler = threading.Thread(target=sample)
        sampler.start()
        steps = []
        read(mgmt)
        for n in range(3):
            serial = requests(mgmt) + 2
            type_prompt(app, f"Resident read {n + 1}")
            app.ax("press", "AXButton", "Send")
            steps.append(("read", settled(app, serial)))
        listing(mgmt)
        serial = requests(mgmt) + 2
        type_prompt(app, "Resident command")
        app.ax("press", "AXButton", "Send")
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Decline") is not None, 15)
        app.ax("press", "AXButton", "Allow")
        steps.append(("command", settled(app, serial)))
        proposal(mgmt, 99)
        serial = requests(mgmt) + 2
        type_prompt(app, "Resident proposal")
        app.ax("press", "AXButton", "Send")
        steps.append(("proposal", settled(app, serial)))
        app.ax("press", "AXButton", "Review")
        steps.append(("review", app.wait_for(lambda t: app.has_text(t, IMPACT), 15)[1]))
        discard_review(app)
        sampler.join()
        end_and_clear(app)
        if len(resident) != 20 or not all(ok for _, ok in steps):
            raise RuntimeError(f"the resident workload did not complete: {steps}, {len(resident)} samples")
        metrics["harness-tools-process-tree-resident"] = {"unit": "MiB", "samples": resident}
        details["resident_steps"] = steps
    finally:
        app.quit()
        fx.sh(["pkill", "-9", "-f", ENGINE])
        endpoint.stop()
        mgmt.stop()
        fx.clean_fixture_root()
        shutil.rmtree(project, ignore_errors=True)
        shutil.rmtree(work, ignore_errors=True)
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
    own = "workload-perf.json" if SUBJECT == "C21" else "workload-perf-C21.json"
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
