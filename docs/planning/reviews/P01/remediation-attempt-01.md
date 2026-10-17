# P01 remediation after Codex review attempt-01

Attempt-01 (thread transport) scored architecture 9, behavior 8,
authority_security 9, validation 8, overall 8 with one required fix and no
evidence gaps or blockers. Its packet, verdict, thread reply and receipt are
unchanged in attempt-01/.

| Finding | Cause | Change | Acceptance |
|---|---|---|---|
| RF01 refused DeleteTenant destroyed scan observability | `DeleteTenant` removed the tenant's scan records before `save_with`; a pre-write storage failure restores only `Data`, so the refused deletion kept the tenant, credential and context but lost its history, and running or queued scans stayed in the operations runtime unobservable and uncancellable (`CancelScan` requires a record) | scan records are dropped only after the deletion is saved; a refused deletion changes nothing a snapshot or `CancelScan` can see | sync_scan `a_refused_tenant_deletion_keeps_its_scans_observable_and_cancellable` (production dispatch/observe, `ScriptedStore::fail_saves`: settled, running and queued scans; refusal keeps tenant, context, credential and all three records; the queued scan cancels NotSent, the running one cancels, the settled outcome stays visible; the accepted deletion then removes tenant and history; nothing is replayed); P01 probe "a refused tenant deletion drops its scan records" (evidence/P01/mutation-probes); full P01 check/test/native/performance/footprint chain rerun on the repaired source |
