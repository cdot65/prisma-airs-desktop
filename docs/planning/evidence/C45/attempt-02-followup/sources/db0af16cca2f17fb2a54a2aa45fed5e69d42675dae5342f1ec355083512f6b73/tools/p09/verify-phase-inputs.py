"""Read-only qualification of current P09 inputs; never a review verdict."""
from pathlib import Path
import argparse
import json
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
import phase_inputs

parser = argparse.ArgumentParser()
parser.add_argument('record', nargs='?', type=Path)
parser.add_argument('--native-only', action='store_true')
args = parser.parse_args()
if args.native_only:
    result = phase_inputs.verify_native(Path.cwd().resolve())
else:
    parser.error('a check-run record is required') if args.record is None else None
    result = phase_inputs.verify(Path.cwd().resolve(), args.record.resolve())
print(json.dumps(result, indent=2))
