"""C24 measurement of the long typed input under an active assistive client
workload against the budgets registered before C24 measured it
(docs/planning/evidence/C24/workload-budget.md, registration in
docs/planning/evidence/C24/workload-budget-registration.json).

  measure_c24.py --label calibration|acceptance

Run from the repository root on the prepared C24 fixture bundle. Only an
acceptance run writes docs/planning/evidence/C24/workload-perf.json, and it
refuses to start unless the budget table matches its registration. Raw
samples are recorded; `xtask native` recomputes every statistic.

- typed-100kib-ax-process-tree-resident: five fresh launches; in each, a
  deterministic 100 KiB prompt is typed into the scan form's Prompt field as
  20-character key events while the accessibility tree is read continuously
  (the active assistive client); the sample is the run's peak RSS summed over
  the app and its WebKit processes (every 0.5 s while typing and 10 s after).
- typed-100kib-ax-key-to-value: in the first run, after the 100 KiB, one key
  event to the field's accessible value showing it (probe.swift `type`),
  20 times.
"""

import argparse
import json
import os
import subprocess
import sys
import tempfile
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, os.path.join(HERE, "..", "native-acceptance"))
import fixture_app as fx  # noqa: E402

ROOT = os.getcwd()
SUBJECT = os.environ.get("AIRS_NATIVE_SUBJECT", "C24")
EVIDENCE = os.path.join(ROOT, "docs/planning/evidence", SUBJECT)
REGISTRATION = os.path.join(ROOT, "docs/planning/evidence/C24/workload-budget-registration.json")
KEY = "SYNTHETIC-C24-PERF-KEY"
RUNS = 5


def sha256_file(path):
    import hashlib
    with open(path, "rb") as f:
        return hashlib.sha256(f.read()).hexdigest()


def utc():
    return time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime())


def webkit_pids():
    return set(int(p) for p in fx.sh(["pgrep", "-f", "com.apple.WebKit"]).stdout.split())


def rss_mib(pids):
    pids = [p for p in pids if p]
    out = fx.sh(["ps", "-o", "rss=", "-p", ",".join(str(p) for p in pids)]).stdout.split()
    return sum(int(v) for v in out) / 1024


class Sampler(threading.Thread):
    """RSS every 0.5 s, with the accessibility tree read in between."""

    def __init__(self, app, pids):
        super().__init__(daemon=True)
        self.app, self.pids = app, pids
        self.samples, self.reads = [], 0
        self.stop = threading.Event()

    def run(self):
        next_rss = 0.0
        while not self.stop.is_set():
            now = time.time()
            if now >= next_rss:
                self.samples.append(rss_mib(self.pids))
                next_rss = now + 0.5
            try:
                self.app.tree()
                self.reads += 1
            except Exception:  # noqa: BLE001 - a read during an update may fail; keep reading
                pass


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--label", choices=["calibration", "acceptance"], required=True)
    args = parser.parse_args()
    registration = json.load(open(REGISTRATION))
    table_sha = sha256_file(os.path.join(ROOT, registration["budgets"]))
    if table_sha != registration["budgets_sha256"]:
        raise SystemExit("the C24 workload table does not match its registration")
    started = utc()
    if started < registration["registered_utc"]:
        raise SystemExit("the clock is before the budget registration")
    prepared = json.load(open(os.path.join(EVIDENCE, "native-prepare.json")))
    bundle = os.path.join(ROOT, prepared["bundle"])
    exe = os.path.join(bundle, "Contents/MacOS/prisma-airs-desktop-v2")
    if sha256_file(exe) != prepared["executable_sha256"]:
        raise SystemExit("prepared executable changed since native-prepare")
    work = tempfile.mkdtemp(prefix="airs-v2-c24-perf-")
    probe = os.path.join(work, "probe")
    built = fx.sh(["swiftc", "-O", os.path.join(HERE, "probe.swift"), "-o", probe])
    if built.returncode != 0:
        raise SystemExit(f"probe did not build: {built.stderr}")
    prompt = os.path.join(work, "prompt-100k.txt")
    block = "The quick brown fox jumps over the lazy dog. "
    with open(prompt, "w") as f:
        f.write((block * (100 * 1024 // len(block) + 1))[:100 * 1024])
    log = []
    app = fx.App(bundle, work, log)
    endpoint = fx.ScanEndpoint(os.path.join(work, "endpoint"), KEY)
    endpoint.install_roots()
    endpoint.start()
    resident, key_to_value, runs = [], [], []
    try:
        app.quit()
        for run in range(RUNS):
            fx.clean_fixture_root()
            before = webkit_pids()
            app.launch()
            time.sleep(2)
            pids = [app.pid, *sorted(webkit_pids() - before)]
            app.open_settings()
            _, created = app.create_custom_tenant("Perf Tenant", endpoint.origin)
            _, saved = app.save_key(KEY)
            if not (created and saved):
                raise RuntimeError("the measurement tenant was not set up")
            app.open_runtime()
            sampler = Sampler(app, pids)
            sampler.start()
            typed = app.type_file("AXTextArea", "Prompt", prompt, timeout=600, per_event=20)
            time.sleep(10)
            sampler.stop.set()
            sampler.join()
            if not typed:
                raise RuntimeError(f"run {run}: the 100 KiB prompt did not arrive")
            resident.append(max(sampler.samples))
            runs.append({"peak_mib": max(sampler.samples), "rss_samples": len(sampler.samples),
                         "ax_reads": sampler.reads, "pids": len(pids)})
            if run == 0:
                app.front()
                app.ax("focus", "AXTextArea", "Prompt")
                time.sleep(0.5)
                out = subprocess.run([probe, "type", str(app.pid), "20"], capture_output=True, text=True)
                if out.returncode != 0:
                    raise RuntimeError(f"input probe failed: {out.stderr.strip()}")
                key_to_value = json.loads(out.stdout)["samples"]
            app.quit()
            time.sleep(2)
    finally:
        fx.sh(["pkill", "-f", fx.PROCESS])
        endpoint.stop()
        fx.clean_fixture_root()
    metrics = {
        "typed-100kib-ax-process-tree-resident": {"unit": "MiB", "samples": resident},
        "typed-100kib-ax-key-to-value": {"unit": "ms", "samples": key_to_value},
    }
    record = {
        "schema": "airs-v2-workload-perf/1",
        "label": args.label,
        "registration_sha256": table_sha,
        "started_utc": started,
        "finished_utc": utc(),
        "executable_sha256": prepared["executable_sha256"],
        "host": fx.sh(["sysctl", "-n", "hw.model"]).stdout.strip(),
        "metrics": metrics,
        "details": {"runs": runs, "log": log},
    }
    own = "workload-perf.json" if SUBJECT == "C24" else "workload-perf-C24.json"
    name = own if args.label == "acceptance" else f"workload-perf-calibration-{started.replace(':', '')}.json"
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
