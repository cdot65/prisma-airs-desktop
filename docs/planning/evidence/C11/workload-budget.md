# C11 and the "large management collections (lists, filters)" workload

The C03 budget catalogue (`docs/architecture/budgets.json`, `later_workloads`)
assigns "large management collections (lists, filters)" to C11. This record
registers its numeric budgets and methods before C11 implements Sessions
investigation or measures anything, under the T20 governance (no
retroactive limits). Host and build under test are those of `budgets.json`
(Mac17,3, Apple M5, macOS 27.0.1; the prepared fixture bundle against a
synthetic localhost TLS management endpoint that answers immediately and
logs its own handling time).

| Budget | Statistic | Limit | Samples | Method |
|---|---|---|---|---|
| sessions-next-page | p95 | 1500 ms | 20 | Fixture build with a management client, Sessions open on a synthetic 24-hour inventory of 2,500 sessions served 25 per page: from AXPress on Next page to the page's 25 session rows and the new range text being exposed in the accessibility tree (tools/perf/probe.swift activate, 2 ms polling). |
| detector-search-1000 | p95 | 15000 ms | 10 | Same endpoint; a detector search that walks the bounded 1,000 sessions (ten pages of 100) because fewer than 100 match: from submitting the search to its completeness statement being exposed in the accessibility tree. |
| sessions-process-tree-resident | max | 450 MiB | 20 | Process-tree RSS (as C03) sampled once per second for 20 s after reading ten Sessions pages and opening ten sessions with their transactions. |

C11 measures these on its prepared fixture bytes and may not change them
without a versioned rationale and reviewer acknowledgement. The C03
`budgets.json` hash is unchanged by this record.
