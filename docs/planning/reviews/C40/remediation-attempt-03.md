# C40 remediation after attempt-03 (9/9/9/8/8; the push to 9)

| Finding | Remediation | Where |
|---|---|---|
| R1 the table judgement guessed the input by text length (the pack's name) | The row is the AXRow holding the action and its regions are its cells in column order: the name (the case's name in the first cell), the input (the third cell's text), the result (the fourth cell's verdict button or "Not run") and the action; a row with fewer than five cells, a missing, collapsed (under 24 CSS px) or overlapping region fails as before. The judgement is recorded with the regions it bound | `acceptance_c40.py` (`table_usable`) |
| E1 the Explain banner had no native evidence | N01 turns the shell's Show the calls switch on, waits for the banner's text, keeps the tree and a capture in each theme, turns the switch off and waits for the text to go; the three facts (absent before, shown, hidden) are in N01's pass rule and the inventory text names the banner | `acceptance_c40.py`, `native-checks.json` |
