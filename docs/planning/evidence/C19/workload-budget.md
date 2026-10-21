# C19 and the "Harness native-engine startup and memory" workload

The C03 budget catalogue (`docs/architecture/budgets.json`, `later_workloads`)
assigns "Harness native-engine startup and memory" to C19. This record
registers its numeric budgets and methods before C19 implements the Harness
lifecycle or measures anything in the app, under the T20 governance (no
retroactive limits).

**Prior information.** Before this registration, one exploratory spike ran
the pinned engine (9331a58, `desktop-server`) outside the app, from a
scratch script against a loopback TLS Responses fixture. It was not this
workload's method and is not evidence for it. Its first launch of a freshly
extracted binary returned the bootstrap receipt in about 2.8 s. Later
launches returned it in about 70 ms, completed a short turn in about 0.3 s,
and the engine's resident memory was about 78 MiB.

**Host and build.** The host and build under test are those of
`budgets.json`: Mac17,3, Apple M5, macOS 27.0.1. The build is the prepared
fixture bundle with the sealed engine as a bundle resource. It runs against
the synthetic inference server (`tools/native-acceptance/mgmt_server.py`)
on loopback, which answers at full speed unless a method below states a
pace.

**Datasets.**
- **Short answer:** a Responses stream of 2 text deltas (about 40 bytes),
  then `response.completed`.
- **Paced answer:** one 32-byte text delta every 50 ms (about 20 s).

**Memory.** The resident limit allows about 100 MiB of engine and headroom
above the 450 MiB process-tree limit that C18 registered for this host.

| Budget | Statistic | Limit | Samples | Method |
|---|---|---|---|---|
| harness-start-ready | p95 | 4000 ms | 10 | From AXPress on Start conversation to the ready state being exposed in the accessibility tree (tools/perf/probe.swift activate, 2 ms polling). Each start launches a new engine: it covers verifying the executable, spawning it, the bootstrap, initialize and thread/start. The first sample is the bundle's first launch. |
| harness-first-output | p95 | 1000 ms | 10 | Short answer. From AXPress on Send to the first assistant text being exposed in the accessibility tree. |
| harness-turn-interrupt | p95 | 1000 ms | 10 | Paced answer, interrupted about 1 s in. From AXPress on Stop answer to the interrupted state being exposed in the accessibility tree. |
| harness-end-reaped | p95 | 2500 ms | 10 | From AXPress on End conversation to the ended state being exposed in the accessibility tree and no engine process remaining (process table polled every 10 ms). |
| harness-responsive-during-turn | p95 | 500 ms | 20 | While a paced answer streams, from AXPress on another destination to that destination being exposed as selected in the accessibility tree. |
| harness-process-tree-resident | max | 550 MiB | 20 | Process-tree RSS (as C03, including the engine), sampled once per second for 20 s. The span covers two conversations: three short answers and one paced answer, interrupted. |

C19 measures these on its prepared fixture bytes. It may not change them
without a versioned rationale and reviewer acknowledgement. This record
leaves the C03 `budgets.json` hash unchanged.
