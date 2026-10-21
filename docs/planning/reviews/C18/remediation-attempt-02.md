# C18 remediation after attempt-02

Verdict attempt-02 scored architecture 9, behavior 8, authority/security 9,
validation 8 and overall 8, with two required fixes, one evidence gap and
no blockers. D034 lets a chunk complete at 8, but the phase needs every
chunk at 9, and R1 shows a factual error in D045. So the chunk is
remediated now instead of being owner-accepted.

## R1: the SDK uploads files; a bounded file subset is supported

D045 said the pinned SDK had no file upload. That was wrong:
`RuntimeClient::create_file` (`scm-sdk-0.1.0/src/airs/gateway/runtime_audio.rs`)
sends a multipart `POST /files`, and it is compiled under the `gateway`
feature this build already enables. D045 is corrected: its matrix now
lists every `RuntimeClient` file operation with its disposition. The C18
milestone's file requirement is met with a bounded subset, not an
exclusion.

**Supported**
- `list_files`: the chosen workspace's files, metadata only (the first
  200; more are reported).
- `create_file`: one file chosen in the native sheet, at most 4 MiB, for
  the `assistants` purpose.
- `delete_file`: one listed file, after an in-app confirmation.
- Attaching one listed file to a Responses run, as an `input_file` beside
  the prompt.

**Not offered, with reasons**
- `retrieve_file`: not needed.
- `download_file`: the app never reads content back.
- The `batch`, `fine-tune` and `vision` purposes: no workflow uses them.
- Vector stores, batches and fine-tuning: no v1 workflow and none in the
  plan.

**Retention and attachment**
- An uploaded file stays in the Gateway until it is deleted, here or
  elsewhere. The app never deletes one by itself, never downloads one, and
  keeps no copy (not in the tenant book, logs or snapshots).
- A run attaches only a file that the workspace's last file read, with
  the same key, listed.

**Authority**
- New `gateway_files` and `authority/gateway_files_domain` modules.
- Every file request is admitted through the same `admit_key` check as a
  run: current context, current workspace reference, the key stored for
  that workspace, only its endpoint.
- One request runs at a time, in the reference's operations domain.
- Replacing or removing the key withdraws the view at once and drops the
  chosen file.
- Rust reads the chosen file once and holds it under a random token, so the
  presentation sees only its name and size.
- One upload takes the choice. Each request is sent once, with a fresh
  trace, and keeps the SDK's `remote_outcome`. An upload or delete without
  a known result is kept on this Mac (tenant book) and is never resent.

**Presentation**
- A Files tab: Read files, Choose file…, Upload, and per-row Delete… with
  a confirmation that takes focus (Cancel returns focus to the row).
- A status line that takes focus.
- The Playground's "Attached file (optional)" choice for Responses runs,
  kept with the draft.
- Kept file changes listed under "Kept on this Mac", offline too.

**Tests**
- Unit:
  - `files_are_read_within_their_bounds`
  - `upload_names_are_ones_a_multipart_part_may_carry`
  - the attachment cases of `runs_and_endpoints_are_bounded`
- Integration:
  - `gateway_files_are_read_uploaded_attached_and_deleted_once_with_the_workspace_key`:
    the key header, no route, multipart bytes, purpose and name, token and
    choice rules, attachment binding, delete once, and the sheet's bounds.
  - `uncertain_file_changes_are_kept_with_their_trace_and_never_resent`:
    400 rejected and not kept; 503, a lost answer and a 502 delete kept;
    no content on disk; offline relaunch; a draft's attachment kept.
  - `another_key_or_workspace_withdraws_the_files_and_their_attachments`.
- App:
  - `reads, uploads and deletes Gateway files once each…`
  - `attaches a file the workspace listed to a Responses run…`
  - `shows file changes without a known result offline…`
- Contract: the files view, chosen file, actions and kept records.
- Native: new C18-N13, plus an offline file record in C18-N09.

## R2: an answer cut short is shown as incomplete

A new run end, `incomplete` (`RunEnd::Incomplete`, `PlaygroundState::Incomplete`),
covers:
- a Responses `response.incomplete` event;
- a chat finish of `length` or `content_filter` (the same defect existed
  there).

The outcome stays completed, because the Gateway answered. Nothing is
retried, and the run is not kept as uncertain.

The status reads "Incomplete: the Gateway ended the answer before it was
finished. The output below is partial." It is followed by the reason, for
example "It reached the output token limit (max_output_tokens)." The
details show "How it ended" for every ended run. Partial output, tokens and
trace stay shown.

**Tests**
- Unit: `cut` in the chat and Responses event tests, and the outcome of
  `Incomplete`.
- Integration: `a_responses_run_streams_once_and_reports_how_it_ended` now
  expects `incomplete` with `max_output_tokens`, usage and trace. It adds a
  chat run cut at `length`, and checks that neither is sent again or kept
  as uncertain.
- App: `tells a complete answer from one the Gateway cut short and from a
  failed one`.
- Native: C18-N12 now also runs an incomplete and a failed Responses run
  and checks the words shown for each.

## G1: causal evidence

New mutation probes:
- an incomplete Responses answer shown completed
- a chat answer cut at its limit shown completed
- the Playground calling an incomplete answer completed
- a run attaching an unlisted file
- an upload sent again
- a file over the bound held for upload
- an unlisted file deleted
- a replaced key leaving the files shown
- an uncertain upload not kept
- a refused upload called uncertain
- a file deleted without asking
- a chosen file carrying more than its name and size
- a chat run carrying the attached file

The native and workload records are rerun against the remediated fixture
build.
