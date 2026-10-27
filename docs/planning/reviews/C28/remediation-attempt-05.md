# C28 remediation after attempt-05 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 a variant change dropped by an overflowing observation stream left the native Go menu stale | Every shell (re)subscription reconciles the native menu with the fresh snapshot's `ui_variant` before forwarding updates, in addition to rebuilding on each observed change; a missed legacy→v2 or v2→legacy transition is therefore repaired on the resubscribe the client performs after `Resync` | `apps/desktop/src-tauri/src/lib.rs` |
| E1 no regression for the recovery path | Host production-path test: dispatch `setUiVariant` through the IPC while no shell observation is live, then open a new shell observation and assert the Go menu now lists Home ⌘1 first (and the legacy list after switching back); native N02 re-recorded on the corrected bundle | `apps/desktop/src-tauri/tests/ipc_admission.rs`, `evidence/C28/native*` |
| Optional | Twenty-tenant popup walk at 200% in both themes recorded (N07); driver docstring corrected | `acceptance_c28.py` |
