#!/usr/bin/env python3
"""Runs one native driver against the C44 bundle and files its record under
the C44 evidence (the cumulative P09 gate): `run_on_v2.py <cNN> [legacy]`.

The driver's own evidence paths (EVIDENCE/NATIVE/RECORD/PARITY) are
redirected to docs/planning/evidence/C44/native/cumulative/<CNN>/, where a
copy of C44's native-prepare.json binds the run to the same executable; a
pre-P09 driver (`legacy`) runs through legacy_v2 (the wizard's placeholder,
activate-on-create, the Go menu for the legacy sidebar entries).
"""

import importlib
import json
import os
import shutil
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

ROOT = os.getcwd()
which = sys.argv[1]
legacy = len(sys.argv) > 2 and sys.argv[2] == "legacy"
chunk = which.upper()
out = os.path.join(ROOT, "docs/planning/evidence/C44/native/cumulative", chunk)
os.makedirs(out, exist_ok=True)
# An earlier run's record never stands for this one: removed before the driver starts, so a crashed run leaves no
# record (the gate reads a missing record as not run, never as the earlier bundle's result).
for stale in ("native-acceptance.json",):
    if os.path.exists(os.path.join(out, stale)):
        os.remove(os.path.join(out, stale))
shutil.copy(os.path.join(ROOT, "docs/planning/evidence/C44/native-prepare.json"), os.path.join(out, "native-prepare.json"))
if legacy:
    importlib.import_module("legacy_v2")
mod = importlib.import_module(f"acceptance_{which}")
mod.EVIDENCE = out
mod.NATIVE = os.path.join(out, "native")
mod.RECORD = os.path.join(out, "native-acceptance.json")
if hasattr(mod, "PARITY"):
    mod.PARITY = os.path.join(out, "parity")
# Trees and the copies of captures the shared helpers write go under this run too. c29.CAPTURES stays the
# fixture app's own capture folder (the system temp dir the app writes "Capture Window" files to): the first
# cumulative run redirected it and every v2 driver's first capture found nothing ("no capture for admin-light").
import acceptance_c29 as c29  # noqa: E402
c29.NATIVE = mod.NATIVE
code = mod.main()
try:
    r = json.load(open(mod.RECORD))
    print("RUN", chunk, [(c["id"], c["result"]) for c in r["checks"]])
except Exception as e:  # noqa: BLE001
    print("RUN", chunk, "no record", e)
sys.exit(code)
