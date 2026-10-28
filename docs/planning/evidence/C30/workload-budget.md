# C30 and the "tenant switch" workload

`docs/planning/chunks/C30.md` requires that after a tenant switch the new
tenant's Home renders from cached scopes within 100 ms, with its read status
pending until its read lands. This record registers the numeric budget and
method before C30 measures it, under the T20 governance (no retroactive
limits).

**Host and build.** The host and build under test are those of
`budgets.json`: Mac17,3, Apple M5, macOS 27.0.1. The build is the prepared
fixture bundle in the v2 presentation (D058), run against the synthetic
management endpoint (`tools/native-acceptance/mgmt_server.py`) on loopback.

**Dataset.** Two tenants on the same synthetic plane: Tenant A read once
(its Inbox holds its items), Tenant B never read. Samples alternate A→B and
B→A, so every switch renders a Home whose scopes differ from the previous one.

| Budget | Statistic | Limit | Samples | Method |
|---|---|---|---|---|
| tenant-switch-home | p95 | 100 ms | 10 | From AXPress on the other tenant's row in the open tenant switcher to the Home heading and the new tenant's name line both being exposed in the accessibility tree (tools/perf/probe.swift activate, 2 ms polling). No read is awaited: the read status may be pending. |

C30 measures this on its prepared fixture bytes. It may not change it
without a versioned rationale and reviewer acknowledgement. This record
leaves the C03 `budgets.json` hash unchanged.
