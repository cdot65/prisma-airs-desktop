# C24 and the "long typed input under an active assistive client" workload

C03's scan-memory diagnosis (`../C03/perf-diagnosis/README.md`) left one
observation open, owned by C24: thousands of small edits to a 100 KiB text
area raise WebKit's UI process memory, and accessibility reads during typing
add to it. C24 must measure long typed input under an active assistive
client and register a budget for it. The budgets, dataset and method are
registered here, before C24 measures this workload, under the T20 governance
(no retroactive limits).

**Prior information.** C24's code was drafted in a scratch copy outside the
repository while C23 was being verified; there only its unit and integration
tests were compiled and run. None of it has been built into a bundle or run
against this workload, and no acceptance measurement of it exists. The
C03 diagnosis (exploratory, on the C03 fixture bundle) observed, summed over
the app and its WebKit processes:
- ready with the prompt focused: 234–235 MiB;
- 100 KiB typed as 20-character key events with accessibility polling:
  680 MiB;
- each further 100 KiB retype with polling: +100–160 MiB, kept after 30 s
  idle (1017 MiB after three retypes).

The app has grown since C03 (C04–C23), and this workload has not been run on
it. The limits below allow for one typing of 100 KiB, which is what the
workload measures; repeated retyping is not a budgeted workload.

**Host and build.** Those of `budgets.json`: Mac17,3, Apple M5, macOS 27.0.1;
the prepared fixture bundle, against the synthetic scan endpoint
(`tools/native-acceptance/fixture_app.py`) on loopback TLS.

**Dataset.** One tenant with a stored scan key, the Runtime Security scan
form, and a deterministic 100 KiB prompt (the sentence "The quick brown fox
jumps over the lazy dog. " repeated, cut to 102,400 characters).

**Method.** Each sample is a fresh launch with no fixture data root. The
prompt is typed into the Prompt field as 20-character System Events key
events while the accessibility tree is read after every event (the active
assistive client: tools/native-acceptance/ax.swift, as the C03 diagnosis did).
Resident memory is ps RSS summed over the app and the WebKit processes that
appeared with its launch, sampled every 0.5 s while typing and for 10 s
after; a sample is that run's maximum. Responsiveness is the time from one
further key event to the Prompt field's accessible value showing it
(tools/perf/probe.swift), taken 20 times after the 100 KiB are typed.

| Budget | Statistic | Limit | Samples | Method |
|---|---|---|---|---|
| typed-100kib-ax-process-tree-resident | max | 900 MiB | 5 | 100 KiB typed as 20-character key events with accessibility reads after each; the run's peak summed RSS (app and its WebKit processes) |
| typed-100kib-ax-key-to-value | p95 | 150 ms | 20 | after the 100 KiB, one key event to the field's accessible value showing it (probe.swift), 20 times in one run |

**Why these limits.** 900 MiB is the diagnosis's 680 MiB for one typing with
polling plus 220 MiB for the app's growth since C03 and run-to-run variation;
it stays well below the 1017 MiB of repeated retyping, which this workload
does not cover. 150 ms keeps one keystroke's feedback within about nine
60 Hz frames in a field already holding 100 KiB under an active assistive
client; the core input-to-paint budget (50 ms) is for an ordinary field
without one.
