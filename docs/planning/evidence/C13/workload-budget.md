# C13 and the "large red-team/model collections" workload

The C03 budget catalogue (`docs/architecture/budgets.json`, `later_workloads`)
assigns "large red-team/model collections" to C13. This record registers its
numeric budgets and methods before C13 implements Red Team or Model/skill
observation or measures anything, under the T20 governance (no retroactive
limits). Host and build under test are those of `budgets.json` (Mac17,3,
Apple M5, macOS 27.0.1; the prepared fixture bundle against synthetic
localhost TLS Red Team, Model Security, AgentGuard and skill endpoints that
answer immediately and log their own handling time).

The datasets follow v1's read bounds: Red Team and Model Security scan lists
read 100 rows; a Red Team report's attack results are read 100 per page up
to ten pages.

| Budget | Statistic | Limit | Samples | Method |
|---|---|---|---|---|
| redteam-scans-open | p95 | 3000 ms | 20 | Fixture build with a management client, Red Teaming observation opened on a synthetic inventory of 100 scans with 30-day statistics: from AXPress on the Red Teaming destination to the 100 scan rows and the statistics source state being exposed in the accessibility tree (tools/perf/probe.swift activate, 2 ms polling). |
| redteam-attacks-1000 | p95 | 15000 ms | 10 | Same endpoint; a scan report whose attack results span ten pages of 100 (1,000 attacks): from AXPress on the report's attack results to its completeness statement being exposed in the accessibility tree. |
| model-summary-open | p95 | 3000 ms | 20 | Same endpoints; the Model and skill observation opened on 100 Model Security scans, AgentGuard 30-day statistics and 100 skills: from AXPress on its destination to every source's state and counts being exposed in the accessibility tree. |
| redteam-model-process-tree-resident | max | 450 MiB | 20 | Process-tree RSS (as C03) sampled once per second for 20 s after opening Red Teaming and Model/skill observation ten times each and reading one 1,000-attack report. |

C13 measures these on its prepared fixture bytes and may not change them
without a versioned rationale and reviewer acknowledgement. The C03
`budgets.json` hash is unchanged by this record.
