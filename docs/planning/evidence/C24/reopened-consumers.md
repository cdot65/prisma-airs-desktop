# C24 reopened-consumer review ledger

C24 closes the cumulative native, privacy and performance matrix. Its source
changes reach code earlier chunks own, so each consumer is listed with what
changed, why its accepted behavior holds, and the evidence re-run on C24's
tree and bundle. Review-protocol.md requires that a change reopen affected
consumers rather than relying on historical scores.

None of these consumers is reopened with `xtask reopen`. That command
invalidates the reopened subject and every later completed subject. No
consumer's accepted behavior or contract changed, and each is re-verified on
this tree by the evidence below, which the C24 review receives.

| Consumer (owner) | What C24 changed | Why its behavior holds | Re-run on C24 |
|---|---|---|---|
| Intent admission (C01, every chunk) | `AuthorityHandle::dispatch` records the admitted intent's kind and any refusal's code in local diagnostics after admission; `Intent::kind` names each variant | Admission, delivery sequencing, effects and results are unchanged; recording happens after effects run, outside the state lock, and never fails a request | full desktop-core suite (every integration target); C01–C23 native drivers on the C24 bundle |
| Local-data reset (C02, C04, C11, C12, C14, C20, C23) | reset also removes the diagnostics folder and records its own outcome | the purge order and its failure stages are unchanged; diagnostics are removed after the import receipts (and C23's launch rollback, D050, runs before any of this), before the offline work | `context_credentials`, `academy_offline`, `imports_rollback` and `native_quality` reset tests; C02-N and C23 native checks |
| Settings (C01, C02, C04) | Settings gains a Diagnostics group after the tenants | the earlier groups, their order and their focus are unchanged | `desktop-shell.test.tsx`, the settings and tenant tests; C01/C02 native drivers (keyboard, zoom, reduced motion) |
| The shell snapshot contract (C01) | `ShellSnapshot.diagnostics` added; the presentation validates it | every earlier field and its validation are unchanged | `snapshot-validation.test.ts`, `invoke.test.ts`, host `ipc_admission` (its fixture adds the view) |
| The host (C01, C19) | a panic hook records the panic's location; adapters name the logs folder | the default panic report still follows; no other adapter changed | desktop-host suite; every native driver |
| Workload measurement tools (C05, C11) | measure_c05.py and measure_c11.py name their record `workload-perf-C05.json` / `workload-perf-C11.json` when measured for another subject, as the later scripts already do | a measurement for their own chunk still writes `workload-perf.json`; the method and samples are unchanged | both re-measured for C24 |
| Versions (C01, C19, C26's packaging) | The app, its crates and packages move from 0.1.0 to 0.2.0, the precursor preview's version (`Cargo.toml`, `tauri.conf.json`, `package.json`; `Cargo.lock` for the three workspace crates only) | No dependency changed; the bundle identifier, data paths and schemas are unchanged, and diagnostics exports name the running version | every native driver on the 0.2.0 bundle; `xtask notices --check` |
| Budget evaluation (C03 and the workloads' owners C05, C11, C13, C15, C18–C22) | xtask evaluates every registered budget for C24; C18's workload added to the controller's list | each owner's own evaluation is unchanged; C18's recorded measurement passes its registered table | xtask suite; C24's re-measurement of every workload |

C18's workload was registered and measured in C18 (`evidence/C18/workload-budget-registration.json`, `workload-perf.json`), but the controller's `WORKLOADS` list did not name it, so neither C18 nor P05 evaluated it. C24's cumulative closure found the gap. The entry is added, and C24 re-measures that workload with every other one on its own bundle.

C24's suites also corrected one earlier chunk's test:

| Consumer (owner) | What C24 changed | Why its behavior holds | Re-run on C24 |
|---|---|---|---|
| `harness_tools::tool_work_still_queued_when_its_turn_is_stopped_is_never_sent` (C21) | The count of management requests the operator's four slow reads are measured against is taken before they are sent, not after | A read can reach the synthetic server before its `send` returns, so the old count could already include some of them and the check "the four reads ran" then failed although all four had run (instrumented: 3 counted before, 4 at the check). It failed intermittently on the committed C23 tree as well (one of two runs of the target). The tested property, that queued tool work is never sent once its turn is stopped, is unchanged | the `harness_tools` target, four consecutive runs, and the full desktop-core suite |

C24's re-measurement also changed the core budgets C03 registered:

| Consumer (owner) | What C24 changed | Why its behavior holds | Re-run on C24 |
|---|---|---|---|
| Core budgets and their measurement (C03; P01's measurement) | budgets.json version 4: the cold-usable-shell method launches each fresh copy once before timing its cold launch, and `measure_c03.py` reports that first launch per run; the version 3 registration is kept as `C03/budget-registration-v3.json` | Limits, statistics, sample counts and every other method are unchanged. C03's and P01's records were measured under version 3 and stand on it; the version 3 failure on C24's bundle is kept (`perf-v3-over-budget-20261023.json`) and its cause is recorded in `perf-diagnosis.md` | `measure_c03.py` and `xtask footprint` under version 4 on the C24 bundle |
