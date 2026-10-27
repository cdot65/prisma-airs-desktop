# C28 remediation after attempt-08 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 palette tenant entries identified by label only | Entries read "Switch to <label> · <environment> · TSG <id>", consistent with the switcher; test with two "Acme" tenants shows both and dispatches the intended id; native N03 asserts the TSG in the palette's tenant entry | `v2/palette.tsx`, `v2-shell.test.tsx`, `acceptance_c28.py` |
| E1 ArrowDown traversal not exercised natively | The active option carries `aria-current`, so the driver walks ArrowDown one row at a time at 100% and 200%, reading the current row after each press (dropped key events are visible), until Settings (row 13 of 33, past the scroll boundary) is current with every current row inside the dialog, then Enter opens Settings; End/Home traversal retained as well; component test asserts ArrowDown moves `aria-current` | `v2/palette.tsx`, tests, `acceptance_c28.py` |
