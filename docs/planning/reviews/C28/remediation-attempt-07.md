# C28 remediation after attempt-07 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 the switcher control's accessible name hid the current tenant | The trigger's name is "Switch tenant: <label> · <environment> · TSG <id>" (or "No tenant yet"), on every destination, expanded or collapsed; component test with two tenants on Harness; native N01/N07 assert the control's own label (Home, Harness, collapsed) | `v2/tenant-switcher.tsx`, `v2-shell.test.tsx`, `acceptance_c28.py` |
| R2 tenant rows showed label and region only | Each choice shows and announces label, environment and TSG, so equal labels stay distinguishable; test with two "Acme" tenants dispatches each id; native N07 checks the TSG text in the collapsed popup | same |
| E1 the zero-tenant Home status was never exercised natively | The driver enables v2 before any tenant exists and asserts "Add a tenant to start reading it." and the switcher's "No tenant yet" name in the AX tree, then creates the twenty tenants from Settings inside v2 and continues | `acceptance_c28.py` |
| Optional staging paths in observations.json | Rewritten on publish | `acceptance_c28.py` |
