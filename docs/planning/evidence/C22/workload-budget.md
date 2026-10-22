# C22 and the "MCP server session" workload

The C21 registration of "tool and MCP workloads" left the MCP server session
workload to C22. Its budgets, datasets and methods are registered here,
before C22's code is built into the app, run or measured, under the T20
governance (no retroactive limits).

**Prior information.** C22's code was drafted in a scratch copy outside the
repository while C21 was being verified. None of it has been built into a
bundle or run against these workloads, and no measurement of them exists.
The limits follow from what C19-C21 measured on this host:
- start to ready: p95 681 ms;
- a renewal restart to first output: p95 342 ms;
- a desktop tool read answered: p95 425 ms;
- the process tree: at most 497 MiB.

To that they add the catalog read through the management client, each MCP
server's Streamable HTTP startup in the engine (initialize and tools/list),
and the MCP token renewal before a restart.

**Host and build.** The host and build under test are those of
`budgets.json`: Mac17,3, Apple M5, macOS 27.0.1. The build is the prepared
fixture bundle with the sealed engine. It runs against the synthetic
management server, Gateway and MCP plane
(`tools/native-acceptance/mgmt_server.py`) on loopback TLS, which answer at
full speed.

**Datasets.**
- **Catalog:** two published servers in Team A (Tools, Docs).
- **Session:** Tools on and signed in; the model asks for its `lookup` tool
  once per turn by the engine's own name for it; the Gateway answers the
  request carrying the tool's output with numbered text.
- **Rotation:** Tools signed in with six-second tokens; each message is
  sent 4 s after the previous answer, so the token is renewed and the
  engine restarted and restored before it is sent.

**Memory.** The resident limit allows 50 MiB above C21's 650 MiB limit for
the engine's MCP client sessions.

All timings use tools/perf/probe.swift `activate`: from AXPress on the named
control to both the named heading and a static text with the named prefix
existing in the accessibility tree (2 ms polling).

| Budget | Statistic | Limit | Samples | Method |
|---|---|---|---|---|
| harness-mcp-catalog-listed | p95 | 2000 ms | 10 | Catalog. From AXPress on "Read the servers again" to the "Workspace MCP servers" heading and Tools' row named with this read's tag (the synthetic catalog's mode "mcpTag"; the name is not part of a server's revision) being exposed: the catalog read through the management client. |
| harness-mcp-session-ready | p95 | 4000 ms | 10 | Session. From AXPress on "Start conversation" to the conversation heading and the "MCP servers: tools (ready)" text (the engine started and its MCP server's Streamable HTTP startup completed); each conversation ended and cleared before the next. |
| harness-mcp-approval-shown | p95 | 2000 ms | 10 | Session. From AXPress on Send to the "Run a workspace MCP tool" text under "Waiting for your approval". |
| harness-mcp-call-answered | p95 | 3000 ms | 10 | Session. From AXPress on "Approve once" to the conversation heading and the Gateway's numbered answer text (the tools/call, its result returned and the answer). |
| harness-mcp-rotation-first-output | p95 | 3000 ms | 10 | Rotation. From AXPress on Send to the conversation heading and the numbered answer text (the MCP token renewal, the engine restart and restore, and the answer). |
| harness-mcp-process-tree-resident | max | 700 MiB | 20 | Process-tree RSS (as C03, including the engine), sampled once per second for 20 s spanning a session start, two approved MCP calls and one rotation. |

C22 measures these on its prepared fixture bytes. It may not change them
without a versioned rationale and reviewer acknowledgement. This record
leaves the C03 `budgets.json` hash unchanged.
