#!/usr/bin/env python3
"""Audit actual P09 phase review with the disclosed C30 owner-closure inputs.

No independent numeric score or completion is assigned by this entrypoint.
"""
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from phase_packet_c32_production import audit

if __name__ == '__main__':
    audit(Path.cwd().resolve(), sys.argv[1])
