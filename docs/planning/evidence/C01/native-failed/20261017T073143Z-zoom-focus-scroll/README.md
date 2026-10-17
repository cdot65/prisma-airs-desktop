# Failed native acceptance run (retained)

Run of `tools/native-acceptance/acceptance_c01.py` on 2026-10-17 (02:31–02:42 CDT,
fixture executable 98f2415c…), after the radio-group driver fix. 10/13 passed;
C01-N09, C01-N10 and C01-N11 failed.

- C01-N10 (app defect): at 200% zoom the driver reached "Dark" by keyboard
  (Tab, Tab, Down, Space) and it became checked, but the page did not scroll
  it into view: its frame [649, 805] size [652, 88] extends past the window
  bottom (857), under the shared frame's fixed 60px decorative fade
  (`native/captures/standard/zoom-200-settings.png`). WebKit does not scroll
  when arrow keys move focus within a radio group. Fix: the desktop shell
  scrolls every newly focused control into view (`block: 'nearest'`) and the
  root scroll padding keeps it clear of the sticky topbar and the fade; a
  Vitest regression test fails when the handler is removed.
- C01-N09 (driver defect): with "Dark" checked, the driver pressed Down to
  reach "Light", but WebKit does not wrap arrow keys at the end of a group,
  so it returned null (`min-size-settings.png`: Dark still focused). Fix: the
  driver chooses Up or Down from the radios' tree order.
- C01-N11 repeats every flow under Reduce Motion, so it failed on the same
  steps. Reduce Motion was restored to its prior (unset) state.
