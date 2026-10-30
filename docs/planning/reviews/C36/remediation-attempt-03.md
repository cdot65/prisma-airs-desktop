# C36 remediation after attempt-03 (9/8/9/8/8, fail: two required fixes)

| Finding | Remediation | Where |
|---|---|---|
| R1 the populated attention row compressed its text and controls at 900×600 / 200% | The row wraps at narrow widths: the text keeps at least 200px (its flex basis) and wraps words rather than clipping, and the action group (the entry's own action, Acknowledge, Snooze) moves under it as a non-shrinking group whose controls keep their 32px; at the board's width the row is still one line, as approved | `home.tsx` |
| R2 / E1 the native check passed the compressed layout | Every layout tree is now measured for the row's usability: the Acknowledge and Snooze controls must be at least 32 CSS px at the row's zoom (64 points at 200%) and the row's title text at least 200 CSS px wide; the N02 verdict requires it, so the compressed layout of attempt-03 fails and only the reflowed one passes, with fresh captures in both themes | `acceptance_c36.py` |
| Optional: the row's own action in the keyboard order | The predicate now names the entry's contextual action between Open the Inbox and Acknowledge | `acceptance_c36.py` |
