# C21 and the "tool and MCP workloads" workload (tools)

The C03 budget catalogue (`docs/architecture/budgets.json`, `later_workloads`)
assigns "tool and MCP workloads" to C21; C22 registers its own MCP server
session workload. This record registers the Harness tool budgets and their
methods before anything of that workload is built into the app, run or
measured, under the T20 governance (no retroactive limits).

**Prior information.** C21's code was drafted in a scratch copy outside the
repository while C20 was being verified. None of it has been built into a
bundle or run against these workloads, and no measurement of them exists.
The limits follow from what C19 and C20 measured on this host:
- start to ready: p95 681 ms;
- first output: p95 158 ms;
- a restore: p95 775 ms;
- the process tree: at most 497 MiB.

To that they add a second Gateway round trip per tool call, a management
read against the synthetic tenant, the engine's sandboxed command, and a
change review's baseline read.

**Host and build.** The host and build under test are those of
`budgets.json`: Mac17,3, Apple M5, macOS 27.0.1. The build is the prepared
fixture bundle with the sealed engine. It runs against the synthetic
management server and Gateway (`tools/native-acceptance/mgmt_server.py`) on
loopback TLS, which answer at full speed. The synthetic Gateway answers
the first request of a turn with the tool call the dataset names, and the
request that carries that call's output with a short answer.

**Datasets.**
- **Read:** the model calls `airs_desktop.inventory` for the topics
  inventory (a page of 3), then answers.
- **Command:** the model calls `exec_command` with `ls` in a project folder
  of 20 small files (read-only), which the engine asks to approve.
- **Proposal:** the model calls `airs_desktop.proposeChange` to update one
  topic's description.

**Memory.** The resident limit allows 50 MiB above C20's 600 MiB limit for
tool results and a change review.

All timings use tools/perf/probe.swift `activate`: from AXPress on the named
control to both the named heading and a static text with the named prefix
existing in the accessibility tree (2 ms polling).

| Budget | Statistic | Limit | Samples | Method |
|---|---|---|---|---|
| harness-tool-read-answer | p95 | 3000 ms | 10 | Read. From AXPress on Send to the conversation heading and the Gateway's numbered answer text ("Answer <n>:"): the Gateway's tool call, the management read through the desktop, the tool result returned and the Gateway's answer. |
| harness-approval-shown | p95 | 1500 ms | 10 | Command. From AXPress on Send to the approval's "Run a command" text being exposed under its "Waiting for your approval" heading. |
| harness-approved-command-done | p95 | 3000 ms | 10 | Command. From AXPress on Allow to the conversation heading and the Gateway's numbered answer text being exposed (the engine's sandboxed run, its output returned and the Gateway's answer). |
| harness-proposal-staged | p95 | 2000 ms | 10 | Proposal. From AXPress on Send to the proposal's summary text being exposed under its "Proposed changes" heading. |
| harness-proposal-review-ready | p95 | 3000 ms | 10 | Proposal. From AXPress on Review to Administration's "Proposed by the Harness: …" heading and the review's impact text being exposed (the review's baseline read). Each review is discarded before the next. |
| harness-tools-process-tree-resident | max | 650 MiB | 20 | Process-tree RSS (as C03, including the engine), sampled once per second for 20 s spanning three reads, one approved command and one proposal reviewed and discarded. |

C21 measures these on its prepared fixture bytes. It may not change them
without a versioned rationale and reviewer acknowledgement. This record
leaves the C03 `budgets.json` hash unchanged.
