# C15 and the "large HTTP bodies and downloads" workload

The C03 budget catalogue (`docs/architecture/budgets.json`, `later_workloads`)
assigns "large HTTP bodies and downloads" to C15. This record registers its
numeric budgets and methods before C15 implements API Client execution or
measures anything, under the T20 governance (no retroactive limits). Host and
build under test are those of `budgets.json` (Mac17,3, Apple M5, macOS
27.0.1; the prepared fixture bundle, whose API Client sends only to loopback,
against a synthetic plain-HTTP loopback server that streams or drains bodies
at full speed and logs its own handling time).

The datasets follow v1's bounds: request bodies up to 64 MiB per file and
128 MiB per multipart body; responses stream to private storage with only a
256 KiB preview held for display. C15 caps one response at 512 MiB.
Bodies stream through bounded buffers in both directions, so no transfer is
held whole in memory. The resident limit allows about 100 MiB above the
433 MiB process tree C13 measured on this host after its own workload.

| Budget | Statistic | Limit | Samples | Method |
|---|---|---|---|---|
| api-client-response-256mib | p95 | 20000 ms | 10 | Fixture build, a saved GET request to a loopback endpoint answering 256 MiB of `application/octet-stream`: from AXPress on Send to the completed response status and size being exposed in the accessibility tree (tools/perf/probe.swift activate, 2 ms polling). |
| api-client-upload-128mib | p95 | 20000 ms | 10 | Same server; a saved multipart POST with two 64 MiB file parts from the collection, drained by the server: from AXPress on Send to the completed response status being exposed in the accessibility tree. |
| api-client-save-response-256mib | p95 | 15000 ms | 10 | The 256 MiB response saved through the staged fixture destination: from AXPress on Save response to the saved state being exposed in the accessibility tree and the file at its full size. |
| api-client-responsive-during-transfer | p95 | 750 ms | 20 | While a 256 MiB response streams: from AXPress on another open request's tab to that tab's URL field being exposed as selected in the accessibility tree. |
| api-client-process-tree-resident | max | 550 MiB | 20 | Process-tree RSS (as C03) sampled once per second for 20 s spanning three 256 MiB responses, one 128 MiB multipart upload and one 256 MiB save. |

C15 measures these on its prepared fixture bytes and may not change them
without a versioned rationale and reviewer acknowledgement. The C03
`budgets.json` hash is unchanged by this record.
