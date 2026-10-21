# C20 and the "protected Harness history" workload

The C03 budget catalogue (`docs/architecture/budgets.json`, `later_workloads`)
assigns "protected Harness history" to C20. This record registers its
numeric budgets and methods before anything of that workload is built into
the app, run or measured, under the T20 governance (no retroactive limits).

**Prior information.** C20's code was drafted in a scratch copy outside the
repository while C19 was being verified; none of it has been built into a
bundle or run against these workloads. No spike or measurement of this
workload exists. The
limits follow from C19's measured Harness lifecycle on this host (start to
ready p95 681 ms, first output p95 158 ms, end and reap p95 61 ms, process
tree at most 496 MiB) plus allowances for the work C20 adds: reading the
engine's replay after a turn, sealing it and replacing a file atomically;
opening sealed records; restoring a replay into a new engine; and a token
refresh at the synthetic identity provider.

**Host and build.** The host and build under test are those of
`budgets.json`: Mac17,3, Apple M5, macOS 27.0.1. The build is the prepared
fixture bundle with the sealed engine. It runs against the synthetic
Gateway and identity provider (`tools/native-acceptance/mgmt_server.py`) on
loopback TLS, which answer at full speed. History keys live in the fixture
Keychain.

**Datasets.**
- **Short answer:** as C19, a Responses stream of 2 text deltas (about 40
  bytes), then `response.completed`.
- **Long conversation:** 50 turns, each a message of about 30 bytes and a
  1 KiB answer (100 transcript entries), saved after every turn.
- **Full history:** 16 saved conversations in one workspace (the
  per-workspace limit C20 sets), all made through the app: two long
  conversations and fourteen of one short turn each.

**Memory.** The resident limit allows 50 MiB above C19's 550 MiB limit for
open and restored history.

All timings use tools/perf/probe.swift `activate`: from AXPress on the named
control to both the named heading and a static text with the named prefix
existing in the accessibility tree (2 ms polling).

| Budget | Statistic | Limit | Samples | Method |
|---|---|---|---|---|
| harness-history-saved | p95 | 2000 ms | 10 | Long conversation, turns 41-50. From AXPress on Send to the Saved conversations list showing the conversation's new message count. Covers the answer, reading the engine's replay, sealing it and replacing the record. |
| harness-history-listed | p95 | 1500 ms | 10 | Full history. After each relaunch, with the workspace chosen again, from AXPress on the Harness destination to the Saved conversations heading and the first long conversation's title. All records are opened to list them. |
| harness-history-opened | p95 | 1000 ms | 10 | Full history, alternating between the two long conversations. From AXPress on a long conversation's Open to its heading and its last answer's text. |
| harness-history-resume-ready | p95 | 5000 ms | 10 | Full history. From AXPress on Continue this conversation, for an opened long conversation, to the conversation heading and "Ready.": verifying and starting a new engine and restoring the replay. Each sample is ended and cleared before the next. |
| harness-refresh-restart-first-output | p95 | 6000 ms | 10 | Company sign-in with 4 s access tokens (renewed 2 s after issue), each sample sent 2.5 s after the last token. From AXPress on Send to the answer's first text: the refresh at the identity provider, reaping the old engine, starting a new one, restoring the conversation and the turn. |
| harness-history-process-tree-resident | max | 600 MiB | 20 | Process-tree RSS (as C03, including the engine), sampled once per second for 20 s while the full history is listed, both long conversations are opened, and one is continued with three short answers. |

C20 measures these on its prepared fixture bytes. It may not change them
without a versioned rationale and reviewer acknowledgement. This record
leaves the C03 `budgets.json` hash unchanged.
