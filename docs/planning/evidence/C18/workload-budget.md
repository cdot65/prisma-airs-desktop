# C18 and the "Gateway streaming and charts" workload

The C03 budget catalogue (`docs/architecture/budgets.json`, `later_workloads`)
assigns "Gateway streaming and charts" to C18. This record registers its
numeric budgets and methods before C18 implements the Playground or the
Gateway observation reads, or measures anything, under the T20 governance
(no retroactive limits).

**Host and build.** The host and build under test are those of
`budgets.json`: Mac17,3, Apple M5, macOS 27.0.1. The build is the prepared
fixture bundle, run against the synthetic management and inference server
(`tools/native-acceptance/mgmt_server.py`) on loopback. That server answers
at full speed unless a method below states a pace.

**Datasets.** They follow v1's Playground bounds: a prompt of up to 32 KiB,
streamed output capped at 64 KiB, and 256 output tokens.

- **Bulk stream:** 64 KiB of text in 2,048 server-sent chunks of 32 bytes
  each, then `[DONE]`.
- **Paced stream:** one 32-byte chunk every 10 ms (about 20 s for 64 KiB).
- **Charts:** a 30-day usage read with 720 hourly points in each of the
  requests, cost, tokens and latency series, plus 200 model-group rows.
- **Logs:** one page of 50 log rows.

**Memory.** The resident limit allows about 120 MiB above the 330 MiB
process tree that C15 measured on this host during its own workload.

| Budget | Statistic | Limit | Samples | Method |
|---|---|---|---|---|
| gateway-stream-first-output | p95 | 750 ms | 10 | Bulk stream. From AXPress on Run in the Playground to the first output text being exposed in the accessibility tree (tools/perf/probe.swift activate, 2 ms polling). |
| gateway-stream-64kib-complete | p95 | 3000 ms | 10 | Bulk stream. From AXPress on Run to the completed state, with all 64 KiB of output, being exposed in the accessibility tree. |
| gateway-stream-stop | p95 | 500 ms | 10 | Paced stream, stopped about 1 s in. From AXPress on Stop to the stopped state being exposed in the accessibility tree. |
| gateway-responsive-during-stream | p95 | 500 ms | 20 | While a paced stream runs, from AXPress on another Gateway tab to that tab being exposed as selected in the accessibility tree. |
| gateway-usage-charts-30d | p95 | 2500 ms | 10 | Charts dataset. From AXPress on Refresh in the Usage tab (30 days) to the totals of all four series and the first model row being exposed in the accessibility tree. |
| gateway-process-tree-resident | max | 450 MiB | 20 | Process-tree RSS (as C03), sampled once per second for 20 s. The span covers three bulk streams, one paced stream stopped mid-way, and two 30-day chart reads. |

C18 measures these on its prepared fixture bytes. It may not change them
without a versioned rationale and reviewer acknowledgement. This record
leaves the C03 `budgets.json` hash unchanged.
