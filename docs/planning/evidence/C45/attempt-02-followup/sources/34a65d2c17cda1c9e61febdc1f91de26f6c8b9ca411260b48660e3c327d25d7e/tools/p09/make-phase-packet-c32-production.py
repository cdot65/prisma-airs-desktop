#!/usr/bin/env python3
"""Build the P09 packet under the explicit source-bound C30 owner closure.

Actual all-9 C31/C32 prerequisites and actual all-9 P09 review are unchanged.
The original packet/verifier files and historical scores remain immutable.
"""
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from phase_packet_c32_production import main

if __name__ == '__main__':
    main()
