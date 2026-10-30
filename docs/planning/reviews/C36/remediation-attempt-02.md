# C36 remediation after attempt-02 (9/9/9/8/8, fail: one required fix)

| Finding | Remediation | Where |
|---|---|---|
| R1 / E1 the 900×600 / 200% layouts were measured after Acknowledge had emptied the attention list | The layout matrix now runs before any hold, while the attention row is populated; at 200% the row's Acknowledge control is focused first so the row is scrolled into view, every layout tree is checked for the row's controls (`attention_populated`) and the N02 verdict requires it in all four rows; the holds (snooze, unsnooze in the Inbox, acknowledge) follow the layouts | `acceptance_c36.py` |
| Optional: keyboard order asserted for two cards and one quick action | The Tab-order predicate now names every card, Open the Inbox, Acknowledge, Snooze and all four quick actions | `acceptance_c36.py` |
