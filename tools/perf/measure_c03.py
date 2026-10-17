#!/usr/bin/env python3
"""C03 performance measurement against the registered budgets
(docs/architecture/budgets.json, registration in
docs/planning/evidence/C03/budget-registration.json).

  measure_c03.py --label calibration|acceptance

Run from the repository root on the prepared C03 fixture bundle. Calibration
runs are exploratory and written to perf-calibration-<time>.json; only an
acceptance run writes docs/planning/evidence/C03/perf.json, and it refuses
to start unless the budgets file matches its registration. Raw samples are
recorded; `xtask native` recomputes every statistic from them.

Workloads (see budgets.json for the exact methods): 20 cold launches from
fresh bundle copies with no fixture data root, 20 warm relaunches, 100
key-to-field samples, 20 s of idle memory, and 20 deterministic 100 KiB
scans with the process tree sampled every 50 ms. The synthetic localhost
TLS endpoint answers every scan; nothing leaves this Mac.
"""

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "native-acceptance"))
import fixture_app as fx  # noqa: E402

ROOT = os.getcwd()
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence/C03")
BUDGETS = os.path.join(ROOT, "docs/architecture/budgets.json")
KEY = "synthetic-c03-perf-key"
HEADING = "Scan a prompt or response"
FRAME_MS = 1000 / 60
MIB = 1024 * 1024
WEBKIT_DIRS = [os.path.join(fx.HOME, "Library/WebKit/io.cdot.prisma-airs-desktop.v2.fixture"),
               os.path.join(fx.HOME, "Library/Caches/io.cdot.prisma-airs-desktop.v2.fixture")]


def sha256_file(path):
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def utc():
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def webkit_pids():
    return set(int(p) for p in fx.sh(["pgrep", "-f", "com.apple.WebKit"]).stdout.split())


def rss_mib(pids):
    if not pids:
        return 0.0
    out = fx.sh(["ps", "-o", "rss=", "-p", ",".join(str(p) for p in pids)]).stdout.split()
    return sum(int(v) for v in out) / 1024


def wait_gone(pids, timeout=10):
    deadline = time.time() + timeout
    while time.time() < deadline:
        alive = [p for p in pids if fx.sh(["ps", "-p", str(p)]).returncode == 0]
        if not alive:
            return True
        time.sleep(0.2)
    return False


class Sampler(threading.Thread):
    def __init__(self, pids, interval):
        super().__init__(daemon=True)
        self.pids, self.interval, self.samples, self.stop = pids, interval, [], threading.Event()

    def run(self):
        while not self.stop.is_set():
            self.samples.append((time.time(), rss_mib(self.pids)))
            time.sleep(self.interval)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", choices=["calibration", "acceptance"], required=True)
    args = parser.parse_args()
    registration = json.load(open(os.path.join(EVIDENCE, "budget-registration.json")))
    budgets_sha = sha256_file(BUDGETS)
    if budgets_sha != registration["budgets_sha256"]:
        raise SystemExit("budgets.json does not match its registration; register a new version first")
    started = utc()
    if started < registration["registered_utc"]:
        raise SystemExit("the clock is before the budget registration")
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256_file(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c03-perf-")
    probe = os.path.join(work, "probe")
    built = fx.sh(["swiftc", "-O", os.path.join(HERE, "probe.swift"), "-o", probe])
    if built.returncode != 0:
        raise SystemExit(f"probe did not build: {built.stderr}")
    log = []
    app = fx.App(bundle, work, log)
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), KEY)
    partial = {"metrics": {}, "details": {}}
    try:
        result = measure(app, endpoint, probe, exe, bundle, work, partial)
    except Exception as error:
        # A failed attempt is kept, never silently discarded or relabelled.
        attempt = os.path.join(EVIDENCE, f"perf-failed-{args.label}-{started.replace(':', '')}.json")
        with open(attempt, "w") as f:
            json.dump({"schema": "airs-v2-perf-failed/1", "label": args.label, "budgets_sha256": budgets_sha,
                       "started_utc": started, "failed_utc": utc(), "error": str(error),
                       "executable_sha256": prepared["executable_sha256"], "partial": partial, "log": log}, f, indent=1)
            f.write("\n")
        raise
    finally:
        fx.sh(["pkill", "-f", fx.PROCESS])
        endpoint.stop()
        fx.clean_fixture_root()
        shutil.rmtree(work, ignore_errors=True)
    hw = fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip()
    record = {
        "schema": "airs-v2-perf/1",
        "label": args.label,
        "budgets_sha256": budgets_sha,
        "started_utc": started,
        "finished_utc": utc(),
        "host": {"model": hw, "os": fx.sh(["sw_vers", "-productVersion"]).stdout.strip() + " (" + fx.sh(["sw_vers", "-buildVersion"]).stdout.strip() + ")"},
        "bundle": prepared["bundle"],
        "executable_sha256": prepared["executable_sha256"],
        "metrics": result["metrics"],
        "details": result["details"],
        "limitations": [
            "Cold launches use fresh bundle copies and no fixture data root, but the OS file cache is not purged (purge needs root).",
            "Input-to-paint is the key-event-to-accessible-value time plus one 60 Hz frame; the compositor frame itself is not captured.",
            "Resident memory is ps RSS summed over the app and the WebKit processes that appeared with its launch; shared pages are counted per process.",
            "The fixture build adds only the Fixture menu and fixture trust roots to production code; footprint is measured on the production build by xtask footprint.",
        ],
        "log": log,
    }
    name = "perf.json" if args.label == "acceptance" else f"perf-calibration-{started.replace(':', '')}.json"
    path = os.path.join(EVIDENCE, name)
    with open(path, "w") as f:
        json.dump(record, f, indent=1)
        f.write("\n")
    for metric, values in record["metrics"].items():
        s = sorted(values["samples"])
        p95 = s[max(1, -(-95 * len(s) // 100)) - 1]
        print(f"{metric}: n={len(s)} median={s[(len(s) + 1) // 2 - 1]:.1f} p95={p95:.1f} max={s[-1]:.1f} {values['unit']}")
    print(f"wrote {os.path.relpath(path, ROOT)}")
    return 0


def launch_probe(probe, exe):
    out = subprocess.run([probe, "launch", exe, HEADING, "15000"], capture_output=True, text=True)
    data = json.loads(out.stdout or "{}")
    if out.returncode != 0 or data.get("ms") is None:
        raise RuntimeError(f"the shell did not become usable: {out.stderr.strip()} {data}")
    return data


def stop_app(app):
    pid = app.running_pid()
    if pid:
        fx.sh(["kill", str(pid)])
        wait_gone([pid])
    fx.sh(["pkill", "-f", fx.PROCESS])


def measure(app, endpoint, probe, exe, bundle, work, partial):
    metrics, details = partial["metrics"], partial["details"]
    stop_app(app)

    # ---- cold: fresh copies, no data root, previous WebKit processes gone ----
    cold, cold_runs = [], []
    for i in range(20):
        fx.clean_fixture_root()
        for d in WEBKIT_DIRS:
            shutil.rmtree(d, ignore_errors=True)
        copy = os.path.join(work, f"cold-{i}", os.path.basename(bundle))
        fx.sh(["ditto", bundle, copy])
        before = webkit_pids()
        data = launch_probe(probe, os.path.join(copy, "Contents/MacOS/prisma-airs-desktop-v2"))
        cold.append(data["ms"])
        time.sleep(1)
        spawned = sorted(webkit_pids() - before)
        cold_runs.append({"ms": data["ms"], "polls": data["polls"], "webkit": len(spawned)})
        stop_app(app)
        wait_gone(spawned)
        shutil.rmtree(os.path.dirname(copy), ignore_errors=True)
    metrics["cold-usable-shell"] = {"unit": "ms", "samples": cold}
    details["cold"] = cold_runs

    # ---- one tenant with a Keychain key on the synthetic endpoint ----
    fx.clean_fixture_root()
    endpoint.install_roots()
    endpoint.start()
    app.launch()
    app.open_settings()
    _, created = app.create_custom_tenant("Perf Tenant", endpoint.origin)
    _, saved = app.save_key(KEY)
    if not (created and saved):
        raise RuntimeError("the measurement tenant was not set up")
    stop_app(app)
    time.sleep(2)

    # ---- warm relaunches ----
    warm, warm_runs = [], []
    for _ in range(20):
        before = webkit_pids()
        data = launch_probe(probe, exe)
        warm.append(data["ms"])
        time.sleep(1)
        spawned = sorted(webkit_pids() - before)
        warm_runs.append({"ms": data["ms"], "polls": data["polls"], "webkit": len(spawned)})
        stop_app(app)
        wait_gone(spawned)
    metrics["warm-usable-shell"] = {"unit": "ms", "samples": warm}
    details["warm"] = warm_runs

    # ---- idle memory, input latency and scans on one warm launch ----
    before = webkit_pids()
    data = launch_probe(probe, exe)
    app.pid = data["pid"]
    time.sleep(3)
    tree = set(webkit_pids() - before)
    pids = [data["pid"], *sorted(tree)]
    details["process_tree"] = {"app": data["pid"], "webkit": sorted(tree),
                               "commands": [fx.sh(["ps", "-o", "comm=", "-p", str(p)]).stdout.strip() for p in sorted(tree)]}
    time.sleep(7)
    idle = []
    for _ in range(20):
        idle.append(rss_mib(pids))
        time.sleep(1)
    metrics["idle-process-tree-resident"] = {"unit": "MiB", "samples": idle}

    app.front()
    app.ax("focus", "AXTextArea", "Prompt")
    time.sleep(0.5)
    out = subprocess.run([probe, "type", str(app.pid), "100"], capture_output=True, text=True)
    if out.returncode != 0:
        raise RuntimeError(f"input probe failed: {out.stderr.strip()}")
    raw = json.loads(out.stdout)["samples"]
    metrics["input-to-paint"] = {"unit": "ms", "samples": [v + FRAME_MS for v in raw]}
    details["input_raw_ms"] = raw

    app.type_into("AXTextField", "Profile name or ID", "Perf profile")
    prompt = os.path.join(work, "prompt-100k.txt")
    block = "The quick brown fox jumps over the lazy dog. "
    with open(prompt, "w") as f:
        f.write((block * (100 * 1024 // len(block) + 1))[:100 * 1024])
    sampler = Sampler(pids, 0.05)
    sampler.start()
    durations = []
    for i in range(20):
        sent = len(endpoint.requests())
        # Paste-like: the 100 KiB arrives as four large edits (budgets v3).
        if not app.type_file("AXTextArea", "Prompt", prompt, timeout=60, per_event=30000):
            raise RuntimeError(f"scan {i}: the 100 KiB prompt did not arrive")
        start = time.time()
        app.ax("press", "AXButton", "Scan", 0)
        _, done = app.wait_for(lambda t: len(endpoint.requests()) == sent + 1 and not any(
            (n.get("label") or "") == "Cancel scan" for n in t) and any(
            n.get("role") == "AXButton" and n.get("label") == "Scan" and n.get("enabled") for n in t), 20)
        if not done:
            raise RuntimeError(f"scan {i} did not settle")
        durations.append((start, time.time()))
    sampler.stop.set()
    sampler.join()
    requests = endpoint.requests()[-20:]
    if not all(r.get("promptChars") == 100 * 1024 and r.get("tokenMatches") for r in requests):
        raise RuntimeError("a scan request did not carry the 100 KiB prompt")
    # Per-scan peak: the maximum over the samples taken from Scan until the
    # verdict settled (and the first sample after it, so no scan is empty).
    per_scan = []
    for start, end in durations:
        window = [v for t, v in sampler.samples if start <= t <= end]
        after = [v for t, v in sampler.samples if t > end][:1]
        per_scan.append(max(window + after))
    metrics["scan-peak-process-tree-resident"] = {"unit": "MiB", "samples": per_scan}
    details["scan"] = {"samples_50ms": len(sampler.samples), "overall_peak_mib": max(v for _, v in sampler.samples),
                       "submit_to_settled_ms": [(end - start) * 1000 for start, end in durations]}
    stop_app(app)
    return {"metrics": metrics, "details": details}


if __name__ == "__main__":
    sys.exit(main())
