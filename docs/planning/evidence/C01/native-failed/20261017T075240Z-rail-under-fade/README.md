# Superseded native acceptance run (retained)

Run of `tools/native-acceptance/acceptance_c01.py` on 2026-10-17 (02:52–03:04 CDT,
fixture executable cf5eb086…). The evaluator then in force reported 13/13, but
inspecting `native/captures/standard/zoom-200-settings.png` showed a defect it
did not test: at 200% zoom the window is narrower than 900 CSS px, so the
shared frame's fixed 60px bottom fade spans the rail as well, and the last
destination ("Settings") reached by Tab sat under it, dimmed. Its frame
[127, 775] size [414, 82] ends exactly at the window bottom (857), which
satisfied "inside the window" but not "clear of the fade" (≤ 857 − 120).

Fix: below 900 CSS px the rail gets bottom padding and scroll padding of
82px so a focused destination scrolls clear of the fade. The evaluator now
requires focused controls to end above the fade (60 CSS px × zoom) for the
200% last destination and radio (C01-N10) and the minimum-size radio
(C01-N09); applied to this run it fails C01-N10 on the last destination.
Reduce Motion was restored to its prior (unset) state.
