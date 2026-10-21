# C18 remediation after attempt-01

Verdict attempt-01 scored architecture 9, behavior 7, authority/security 8,
validation 7 and overall 7. It listed five required fixes, two evidence
gaps and no blockers. Behavior and validation are below the D034 chunk floor
of 8, so the chunk is remediated before completion. D044 is amended, D045
is new, and the DESIGN C18 section records the decisions below.

## R1: Playground drafts are edited offline, and pending edits are kept

`SavePlaygroundDraft` now takes the workspace reference as optional:

- With the draft's local `id` and no workspace, it edits a kept draft
  under the workspace it was kept for. That needs no workspace listing or
  reference, so it works offline.
- With a workspace and no `id`, it keeps the chosen workspace's draft, as
  before.
- Anything else (an unknown ID, or both an ID and a workspace) is refused.

"Kept on this Mac" offers **Edit** for each Playground draft. Edit opens
an inline editor (API, model, route ID, system text, prompt) that saves
by ID. **Done editing** returns focus to the Edit button. Running is still
only from the Playground of a workspace chosen from a fresh listing, with
its stored key, and nothing is replayed.

A shared keeper now saves a pending edit when the form loses focus or
closes (and before a run), not only after the 800 ms pause. Once a kept
draft exists, the Playground also saves it by its ID, so a workspace
change cannot leave the edit under a retired reference.

Tests:
- Integration `playground_drafts_and_uncertain_runs_survive_an_offline_restart_and_nothing_is_replayed`
  now does all of this on an offline relaunch: it edits the kept draft by
  ID (prompt, API, system), checks the refusals, relaunches offline again
  and finds the edit.
- App: `edits a kept Playground draft offline by its ID…` (no listing, no
  workspace chosen; the save carries the ID and no workspace; focus
  returns) and `keeps a Playground edit made just before leaving the tab`.
- Native C18-N09 now edits the draft offline by keyboard, relaunches
  offline again, and opens the edit in the Playground once online.

## R2: opening failures keep the SDK's outcome

When the stream cannot be opened, the run records the SDK error's
`remote_outcome()` and settles to it: `NotSent`, `Rejected` or `Unknown`.
The failure label no longer decides anything: a 5xx is `unavailable` and
unknown, and a 4xx is rejected. Unknown runs are kept on this Mac with
their trace, as before.

Tests:
- The stream test's table now covers 446, 429, 401 and 403 (rejected),
  500, 502, 503 and 504 (unknown), a lost handshake (unknown, `network`)
  and a malformed successful handshake (unknown, `invalidResponse`). Each
  is sent once.
- New `uncertain_openings_keep_their_trace_across_a_restart_and_are_never_replayed`:
  every unknown opening is kept with its trace and a 4xx is not. All of
  them are restored on an offline relaunch, and nothing is replayed.
- Unit `outcomes_follow_what_was_sent_and_answered` checks that the label
  never decides the outcome.
- Native C18-N04 adds a 503 run: it is named, said to be possibly charged,
  kept with its trace, and sent once.

## R3: a retired key withdraws its run at once

`retire_inference` marks the tenant's run as retired, even after it has
settled, and cancels it if it is still going. A single `run_bound` check
is used both for the view and for settlement: the run is not retired, it
is under the active context and the chosen workspace, and the key record
is the one it was admitted with. The snapshot published at admission of a
key replacement or removal therefore no longer carries the run or its
output. Its settlement still keeps the sanitized record.

Test: new `a_replaced_or_removed_key_withdraws_its_run_from_every_snapshot_at_once`.
- It subscribes before the change and drains every published snapshot and
  direct observation while the cancellation runs. None of them carries the
  run or its output.
- It checks that the stream is closed and the record is kept, that the
  context and the workspace choice stay, and that the next run sends only
  the new key.

## R4: log pages are bound to their query, and a capped list is not complete

`GatewayLogs` gains `capped`. It is true on the last page that may be read
when more rows are reported, or when that page is full and has no total;
`more` is then false. The contract refuses `more` and `capped` together.

`LogsPanel` reads a page of an explicit query:
- Read logs applies the form.
- Read more logs continues the query of the pages shown (its window, trace
  and status), never the edited form.
- A reply commits only if it is the latest read, answers that page and
  query, and continues the same query's list.

The status line names the query shown. A capped list says "only the first
1,000 are read here: more exist" and never "all read".

Tests:
- Unit `logs_belong_to_the_chosen_workspace…` checks capped pages with a
  total and without one.
- The integration observation test reads page 39 with 5,000 reported.
- App `continues the logs of the query shown…`:
  1. It edits the trace without applying it, presses Read more, and checks
     the continuation request.
  2. It applies the filter while that page is still out; the late page is
     dropped.
  3. A capped list offers no Read more and never says all read.

## R5: Gateway holds and completeness are per workspace and known

`GatewayOverview::activity_known` is true only when the day's requests and
errors were both read, whole and reported. If they were not, the gap
"Gateway requests or errors not reported" keeps the face off "All clear".

Hold reconciliation:
- A Gateway hold (`gateway-errors-<workspace>`) ends only when the read's
  own workspace has a known day and no longer reports it. A read of
  another workspace, or an unknown day, keeps it.
- Gateway holds no longer depend on the Runtime sources being complete.
  Runtime holds keep their rule.

Test: new `a_gateway_hold_ends_only_from_a_known_day_of_its_own_workspace`:
1. A is acknowledged, then B is read known and clean, then A is read
   again: A's item is still held.
2. A day of A with unreported errors is a gap, never "All clear", and
   keeps the hold.
3. A known clean day of A ends the hold.
4. A late read of B that settles after A was chosen again is labelled B
   and leaves A's snooze.

## G1: response and file workflows

D045 compares the pinned SDK's `RuntimeClient` with v1 operation by
operation:

- **Responses: now supported.** The Playground offers Chat completions
  (the default, v1's API) or Responses. A Responses run is a streamed
  `/responses` run under every D044 rule: same key, binding, headers,
  bounds, single send, outcomes and records. It is sent with
  `store: false` and `max_output_tokens` 256. Text deltas are output; the
  completed or incomplete event gives the ID, model, tokens and end; a
  failed event is `modelFailed`.
- **Files: not offered, with the reason recorded.** The pinned SDK has no
  file upload, v1 had no file workflow, and a file the Gateway already
  holds has no defined retention or attachment rules.

Tests:
- Unit `responses_events_carry_text_identity_usage_and_end`; the request
  test covers the Responses body.
- Integration `a_responses_run_streams_once_and_reports_how_it_ended`
  covers completed, failed and incomplete runs, the route, trace and key
  headers, `store: false`, and that nothing goes to chat completions.
- App `runs with the Responses API when chosen`.
- Native: new C18-N12.

## G2: causal evidence

The integration, App and native tests above exercise each fix through the
production authority interface and App. New mutation probes break each
guard and require its named test to fail:
- 5xx called a refusal
- retired key's run still shown
- kept draft not editable offline
- pending edit dropped on leaving
- offline edit naming a workspace
- more logs following the edited filters
- a page of another query joining
- a capped list called complete
- another workspace's read ending a hold
- unreported errors ending a hold
- a Responses run stored
- a failed response shown completed

The native and workload records were rerun against the remediated
fixture build.
