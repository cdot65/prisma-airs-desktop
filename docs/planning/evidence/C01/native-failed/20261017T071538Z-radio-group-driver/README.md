# Failed native acceptance run (retained)

Run of `tools/native-acceptance/acceptance_c01.py` on 2026-10-17 (02:07–02:30 CDT,
fixture executable 98f2415c…). 11/13 checks passed; C01-N10 and C01-N11 failed.

Cause: a driver defect, not an app defect. `operateRadio('Dark')` at 200% zoom
pressed Tab looking for a focused "Dark" radio, but a radio group is one Tab
stop that lands on its checked radio ("Light" at that point), so "Dark" is
never a Tab stop. The driver wrapped through the window and returned null
(see `native/captures/standard/zoom-200-settings.png`: Light checked, focus
ring back on the first destination). N11 repeats every flow under Reduce
Motion, so it failed on the same step.

Fix: the driver now tabs into the group and uses Down Arrow to reach the named
radio before Space, recording the keys used; the evaluator also requires the
operated radio to be the requested one (Light at minimum size, Dark at 200%).
Reduce Motion was restored to its prior (unset) state by the script.
