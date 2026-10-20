#!/usr/bin/env python3
"""C15 measurement of the large HTTP bodies and downloads workload against
the budgets registered before C15 implemented it
(docs/planning/evidence/C15/workload-budget.md, registration in
docs/planning/evidence/C15/workload-budget-registration.json).

  measure_c15.py --label calibration|acceptance

Run from the repository root on the prepared C15 fixture bundle. Only an
acceptance run writes docs/planning/evidence/<subject>/workload-perf.json,
and it refuses to start unless the budget table matches its registration.
Raw samples are recorded; `xtask native` recomputes every statistic.

Workloads, on one launch against the synthetic plain-HTTP loopback endpoint
(tools/native-acceptance/workbench_server.py), which streams or drains at
full speed:

- api-client-response-256mib: from AXPress on Send (a saved GET answering
  256 MiB of octet-stream) to the response summary "HTTP 200 · 268.4 MB"
  existing in the Accessibility tree (tools/perf/probe.swift activate, 2 ms
  polling). A new Send clears the previous response first.
- api-client-upload-128mib: from AXPress on Send (a saved multipart POST of
  two 64 MiB collection files, drained by the server) to "HTTP 200 ·".
- api-client-save-response-256mib: from AXPress on Save response… to
  "Saved 268.4 MB as save-N.bin." (the staged fixture destination, a new
  name per sample), the file then checked at its full size.
- api-client-responsive-during-transfer: while a 256 MiB response streams
  (paced by the server to about four seconds), from AXPress on another open
  request's tab to that tab's URL field exposing its URL (probe.swift
  activate-field, the registered editor-ready target); each sample is
  checked to have ended before the stream did.
- api-client-process-tree-resident: the app plus its WebKit processes' RSS
  sampled once per second for 20 s spanning three 256 MiB responses, one
  128 MiB multipart upload and one 256 MiB save.
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
import acceptance_c15 as c15  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C15")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
REGISTRATION = os.path.join(ROOT, "docs/planning/evidence/C15/workload-budget-registration.json")
BIG = 256 * 1024 * 1024
PART = 64 * 1024 * 1024
BIG_DONE = "HTTP 200 · 268.4 MB"


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


def probe_field(probe, app, role, label, field, value, timeout=60000):
    out = fx.sh([probe, "activate-field", str(app.pid), role, label, field, value, str(timeout)])
    return json.loads(out.stdout or "{}")


def stage(work, server):
    root = os.path.join(os.path.realpath(work), "repo", "api")
    os.makedirs(os.path.join(root, "data"))
    os.makedirs(os.path.join(root, "..", ".git"))
    with open(os.path.join(root, "collection.toml"), "w") as f:
        f.write(f'format_version = 1\nid = "c15perf"\nname = "Perf"\n\n[variables]\nbase = "{server.origin}"\n')
    for name in ("a.bin", "b.bin"):
        with open(os.path.join(root, "data", name), "wb") as f:
            f.truncate(PART)
    files = {
        "big": c15.request("big", "GET", f"{{{{base}}}}/big?bytes={BIG}"),
        "paced": c15.request("paced", "GET", f"{{{{base}}}}/big?bytes={BIG}&pace=1000"),
        "items": c15.request("items", "GET", "{{base}}/items"),
        "upload": c15.request("upload", "POST", "{{base}}/upload", 'body_kind = "multipart"\nbody = "upload.body.txt"\n'),
    }
    for name, text in files.items():
        with open(os.path.join(root, f"{name}.request.toml"), "w") as f:
            f.write(text)
    with open(os.path.join(root, "upload.body.txt"), "w") as f:
        f.write('[[parts]]\nname = "a"\nfile = "data/a.bin"\n[[parts]]\nname = "b"\nfile = "data/b.bin"\n')
    os.makedirs(fx.DATA, exist_ok=True)
    with open(os.path.join(fx.DATA, "fixture-open-folder"), "w") as f:
        f.write(root + "\n")
    return root


def stage_save(work, n):
    path = os.path.join(work, "saved", f"save-{n}.bin")
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(os.path.join(fx.DATA, "fixture-save-response"), "w") as f:
        f.write(path + "\n")
    return path


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", choices=["calibration", "acceptance"], required=True)
    label = parser.parse_args().label
    registration = json.load(open(REGISTRATION))
    table = os.path.join(ROOT, registration["budgets"])
    table_sha = sha256_file(table)
    if table_sha != registration["budgets_sha256"]:
        raise SystemExit("the workload budget table does not match its C15 registration")
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256_file(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c15-perf-")
    probe = os.path.join(work, "probe")
    built = fx.sh(["swiftc", "-O", os.path.join(HERE, "probe.swift"), "-o", probe])
    if built.returncode != 0:
        raise SystemExit(f"probe did not build: {built.stderr}")
    started = utc()
    app = fx.App(bundle, work, [])
    app.quit()
    fx.clean_fixture_root()
    server = c15.Workbench(os.path.join(work, "workbench"))
    stage(work, server)
    metrics, details = {}, {}
    try:
        before = webkit_pids()
        app.launch()
        time.sleep(2)
        pids = [app.pid, *sorted(webkit_pids() - before)]
        details["process_tree"] = {"app": app.pid, "webkit": pids[1:]}
        c15.open_client(app)
        c15.press(app, "Open collection…")
        app.wait_for(lambda t: fx.App.node(t, "AXButton", "Open GET big.request.toml") is not None, 10)

        # ---- api-client-response-256mib ----
        c15.open_request(app, "big", "GET")
        timings, runs = [], []
        for n in range(10):
            mark = len(server.requests("/big", ends=True))
            data = probe_activate(probe, app, "AXButton", "Send", "big", BIG_DONE)
            ends = server.requests("/big", ends=True)[mark:]
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "streamed": [e["bytes"] for e in ends], **data})
            time.sleep(0.5)
        if any(t is None for t in timings) or any(r["streamed"] != [BIG] for r in runs):
            raise RuntimeError(f"a 256 MiB response did not complete: {runs}")
        metrics["api-client-response-256mib"] = {"unit": "ms", "samples": timings}
        details["response_256mib"] = runs

        # ---- api-client-save-response-256mib ----
        timings, runs = [], []
        for n in range(10):
            path = stage_save(work, n + 1)
            data = probe_activate(probe, app, "AXButton", "Save response…", "big", f"Saved 268.4 MB as save-{n + 1}.bin.")
            size = os.path.getsize(path) if os.path.exists(path) else None
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "bytes": size, **data})
            if os.path.exists(path):
                os.remove(path)
            time.sleep(0.4)
        if any(t is None for t in timings) or any(r["bytes"] != BIG for r in runs):
            raise RuntimeError(f"a response was not saved in full: {runs}")
        metrics["api-client-save-response-256mib"] = {"unit": "ms", "samples": timings}
        details["save_256mib"] = runs

        # ---- api-client-upload-128mib ----
        c15.open_request(app, "upload", "POST")
        timings, runs = [], []
        for n in range(10):
            mark = len(server.requests("/upload"))
            data = probe_activate(probe, app, "AXButton", "Send", "upload", "HTTP 200 ·")
            got = server.requests("/upload")[mark:]
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "received": [g["bytes"] for g in got], **data})
            time.sleep(0.5)
        if any(t is None for t in timings) or any(len(r["received"]) != 1 or r["received"][0] < 2 * PART for r in runs):
            raise RuntimeError(f"an upload was not received in full: {runs}")
        metrics["api-client-upload-128mib"] = {"unit": "ms", "samples": timings}
        details["upload_128mib"] = runs

        # ---- api-client-responsive-during-transfer ----
        c15.open_request(app, "items", "GET")
        c15.open_request(app, "paced", "GET")
        timings, runs = [], []
        for n in range(20):
            mark = len(server.requests("/big", ends=True))
            c15.press(app, "Send")
            time.sleep(0.3)
            clicked = time.time()
            data = probe_field(probe, app, "AXRadioButton", "GET items", "URL", "{{base}}/items", 10000)
            answered = clicked + (data.get("ms") or 0) / 1000
            app.ax("press", "AXRadioButton", "GET paced")
            app.wait_for(lambda t: any(x.get("role") == "AXHeading" and x.get("label") == "paced" for x in t), 10)
            app.wait_for(lambda t: len(server.requests("/big", ends=True)) > mark, 30)
            ends = server.requests("/big", ends=True)[mark:]
            during = bool(ends) and answered < ends[0]["at"]
            timings.append(data.get("ms"))
            runs.append({"sample": n + 1, "during_transfer": during, **data})
            app.wait_for(lambda t: not app.has_text(t, "Sending GET…"), 30)
            time.sleep(0.3)
        if any(t is None for t in timings) or not all(r["during_transfer"] for r in runs):
            raise RuntimeError(f"a tab switch was not measured during a transfer: {runs}")
        metrics["api-client-responsive-during-transfer"] = {"unit": "ms", "samples": timings}
        details["responsive"] = runs

        # ---- api-client-process-tree-resident ----
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
        c15.open_request(app, "big", "GET")
        for _ in range(3):
            c15.press(app, "Send")
            _, ok = app.wait_for(lambda t: app.has_text(t, BIG_DONE), 60)
            steps.append(("response", ok))
        path = stage_save(work, 99)
        c15.press(app, "Save response…")
        _, ok = app.wait_for(lambda t: app.has_text(t, "Saved 268.4 MB as save-99.bin."), 60)
        steps.append(("save", ok))
        if os.path.exists(path):
            os.remove(path)
        c15.open_request(app, "upload", "POST")
        c15.press(app, "Send")
        _, ok = app.wait_for(lambda t: app.has_text(t, "HTTP 200 ·"), 60)
        steps.append(("upload", ok))
        sampler.join()
        if len(resident) != 20 or not all(ok for _, ok in steps):
            raise RuntimeError(f"the resident workload did not complete: {steps}, {len(resident)} samples")
        metrics["api-client-process-tree-resident"] = {"unit": "MiB", "samples": resident}
        details["resident_steps"] = steps
        details["requests"] = len(server.requests())
    finally:
        app.quit()
        server.stop()
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
    name = "workload-perf.json" if label == "acceptance" else f"workload-perf-calibration-{started.replace(':', '')}.json"
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
