# C28 remediation after attempt-06 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 tenant-scoped refusals did not name the tenant | The switch and read actions capture the target tenant's label at dispatch time and the refusal/unconfirmed messages carry it ("The switch to Meridian Retail was refused (…)", "The read of Northwind Health was refused (…)"), also when the active context changes before the response; test "names the intended tenant in a refused switch and the read tenant in a refused read" | `app.tsx`, `v2/copy.ts`, `v2-shell.test.tsx` |
| E1 stale native record (driver crashed early after deleting the evidence folder; the previous all-pass record remained) | The driver stages each run in `native.staging/` and publishes only a complete run (evidence folder and record together); control presses wait for the node to appear; the chain gate and the packet builder verify the record's executable hash against `native-prepare.json` and the bundled executable, that every referenced evidence file exists, and that every check passed, before anything is submitted | `acceptance_c28.py`, chain and packet scripts |
| Optional | As above (staging + validation) | |
