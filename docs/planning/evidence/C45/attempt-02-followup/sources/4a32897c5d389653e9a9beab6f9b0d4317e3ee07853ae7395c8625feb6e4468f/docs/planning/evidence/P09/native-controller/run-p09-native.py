"""Redirect a retained native driver to P09 evidence without changing checks.

Run in the approved GUI context only. The legacy adapter and assertion-level
requirements are the same ones C44 used. Every record must name the exact P09
prepared executable; raw records keep their original generated attribution.
"""
from pathlib import Path
import datetime
import hashlib
import importlib
import json
import os
import platform
import shutil
import subprocess
import sys

ROOT = Path.cwd().resolve()
TOOLS = ROOT / 'tools/native-acceptance'
sys.path.insert(0, str(TOOLS))
sys.path.insert(0, str(Path(__file__).resolve().parent))
from native_requirements import load_requirements, assess_record


def main():
    which = sys.argv[1]
    assert which in ['c03','c04','c05','c06','c07','c10','c11','c24','c31',*[f'c{i}' for i in range(35,45)]]
    legacy = len(sys.argv) > 2 and sys.argv[2] == 'legacy'
    chunk = which.upper()
    phase = ROOT / 'docs/planning/evidence/P09'
    prepared = json.loads((phase / 'native-prepare.json').read_text())
    exe = ROOT / prepared['bundle'] / 'Contents/MacOS/prisma-airs-desktop-v2'
    assert hashlib.sha256(exe.read_bytes()).hexdigest() == prepared['executable_sha256']
    out = phase if chunk == 'C44' else phase / 'native/cumulative' / chunk
    out.mkdir(parents=True, exist_ok=True)
    record = out / 'native-acceptance.json'
    assert not record.exists(), 'preserve earlier native records; archive explicitly before retry'
    if chunk != 'C44':
        shutil.copy2(phase / 'native-prepare.json', out / 'native-prepare.json')
    if legacy:
        importlib.import_module('legacy_v2')
    mod = importlib.import_module(f'acceptance_{which}')
    original = str(mod.EVIDENCE)
    # Only relocate evidence paths. Do not alter driver assertions, flows,
    # timeouts, result values, provenance strings or the system capture folder.
    for name in ['EVIDENCE', 'NATIVE', 'RECORD', 'PARITY']:
        if hasattr(mod, name):
            value = getattr(mod, name)
            assert value == original or value.startswith(original + os.sep)
            setattr(mod, name, str(out) + value[len(original):])
    c29 = importlib.import_module('acceptance_c29')
    c29.NATIVE = mod.NATIVE
    if '--dry-run' in sys.argv[2:]:
        required = load_requirements(ROOT)[chunk]
        print(json.dumps({'subject':chunk,'mode':'paths only; no UI or driver execution',
                          'evidence':str(out),'required_checks':sorted(required),
                          'executable_sha256':prepared['executable_sha256']}))
        return 0
    started = datetime.datetime.now(datetime.timezone.utc).isoformat()
    os.environ['AIRS_NATIVE_ORCHESTRATOR'] = 'Codex'
    code = mod.main()
    raw = json.loads(record.read_text())
    errors, carried = assess_record(raw, ROOT, prepared['executable_sha256'], load_requirements(ROOT)[chunk])
    execution = {'schema':'airs-v2/p09-native-execution/1','phase':'P09','driver_subject':chunk,
                 'orchestrator':'Codex','started_utc':started,
                 'finished_utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),
                 'source_root':str(ROOT),'source_head':subprocess.check_output(['git','rev-parse','HEAD'],text=True).strip(),
                 'os':platform.platform(),'driver_return_code':code,
                 'executable_sha256':prepared['executable_sha256'],
                 'raw_record_sha256':hashlib.sha256(record.read_bytes()).hexdigest(),
                 'controller_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
                 'requirements_sha256':hashlib.sha256(Path(__file__).with_name('native_requirements.py').read_bytes()).hexdigest(),
                 'driver_sha256':hashlib.sha256((TOOLS / f'acceptance_{which}.py').read_bytes()).hexdigest(),
                 'required_check_errors':errors,'superseded_raw_checks':carried,
                 'adaptation':'evidence paths relocated to P09; no assertion or result changes',
                 'raw_attribution':'driver-generated performed_by retained; C43 reads the actual orchestrator from AIRS_NATIVE_ORCHESTRATOR'}
    (out / 'execution.json').write_text(json.dumps(execution,indent=2)+'\n')
    print(json.dumps({'subject':chunk,'required_pass':not errors,'errors':errors,'carried':carried}))
    return 1 if errors else 0


if __name__ == '__main__':
    raise SystemExit(main())
