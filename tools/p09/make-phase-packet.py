#!/usr/bin/env python3
"""P09 phase packet entry point. All current inputs must qualify before writes.

The former disabled draft is immutable under evidence/P09/phase-tooling-history.
This builder remains unable to create a packet until current native/check gates
and full-9 predecessors pass; C41 override and C33 supersession stay explicit.
"""
from pathlib import Path
import sys

sys.path.insert(0, str(Path(__file__).resolve().parent))
from phase_packet import main

if __name__ == '__main__':
    main()
