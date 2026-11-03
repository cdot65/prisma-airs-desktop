"""Audit an actual P09 phase verdict against its frozen current-input packet."""
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from phase_packet import audit

if __name__ == '__main__':
    audit(Path.cwd().resolve(), sys.argv[1])
