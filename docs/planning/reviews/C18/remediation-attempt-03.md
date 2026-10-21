# C18 remediation after attempt-03

Verdict attempt-03 scored architecture 9, behavior 8, authority/security 9,
validation 8 and overall 8, with three required fixes, one evidence gap and
no blockers. Each is a boundary case of the D045 file subset. They are
remediated now, so the chunk reaches the phase bar without a later raise.

## R1: uploads keep the list's bound

After a completed upload, `sync_files` puts the new file at the top of the
list. If the list then exceeds `MAX_FILES` (200):
- it is cut to 200;
- `more` is set, so the files no longer shown are reported, never dropped
  silently;
- the new file stays shown and usable.

This covers a full list, a list that was already cut, and repeated uploads.
The snapshot therefore always meets the contract's bound of 200 rows.

Tests:
- Integration `file_lists_keep_their_bound_deletes_their_identity_and_times_their_range`
  runs through the Core upload-to-snapshot path:
  - 200 listed files, then two uploads: each time 200 rows, the new file
    first, `more` true;
  - 201 listed files (cut to 200), then an upload: 200 rows, `more` true.
- App `keeps the Files view usable with a full list after uploads and times no date can render`:
  the 200-row snapshot with `more`, pushed through the client's own
  validation, renders 200 rows and "200 files shown: the Gateway holds
  more…".
- Contract: 200 rows with `more` are accepted; 201 are refused.

## R2: a delete counts only when its answer names the file

`perform` compares the answer's `id` with the requested file. A mismatch is
`InvalidResponse` with outcome `Unknown`, whatever `deleted` says. The
change was sent, so:
- it is kept on this Mac with its trace;
- the file stays in the cached list;
- it is never resent.

A matching `deleted: false` stays a refusal, and a matching `deleted: true`
completes. For a file request the failure reads "The Gateway’s answer did
not describe that file.", both in the status and in the kept record.

Tests:
- The integration test above uses literal answers:
  - another file's ID with `deleted: true` and with `deleted: false`: each
    is unknown and `invalidResponse`, kept with its trace, and the file is
    still listed;
  - the same ID with `deleted: false` (rejected) and with `deleted: true`
    (completed, file gone);
  - four deletes, each sent once.
- Native C18-N13 adds a delete answered for another file. The status says
  the result is not known and why, the change is kept under "Kept on this
  Mac", the row stays, and one DELETE was sent.

## R3: creation times no date can render

`GatewayFile.created_at` is now `Option<u64>`:
- A finite, non-negative time up to 8,640,000,000,000 seconds (the end of
  the JavaScript date range) is kept.
- A later one is `null`, an unknown time, and the file stays listed and
  usable.
- An absent, negative or non-numeric time is still malformed.

On the presentation side:
- The contract accepts `null` or an integer within that range.
- `createdText` shows "Unknown time" for `null` or any unrenderable value,
  and writes extended years with their sign; it never throws.

Tests:
- Unit `files_are_read_within_their_bounds`: a late time, the edge value,
  and malformed times.
- Integration: a list with a time of 9e12 seconds is read without failure,
  that file's `createdAt` is null, and the file is then deleted.
- App:
  - `createdText` for ordinary, edge, late and null times;
  - the Files view rendering "Unknown time" and deleting that file.
- Contract: the edge value and `null` are accepted; a time past the edge
  and a negative time are refused.
- Native C18-N13 lists a file with a time of 9e12 seconds as "Unknown
  time", with its Delete control.

## G1: causal evidence

New mutation probes (each kills its named test):
- an upload grows the list past its bound
- an upload into a full list hides what it drops
- a delete answered for another file settles this one
- a time no date can render refuses the list
- a time no date can render throws in the Files view
- the contract accepts a time no date can render
- the contract refuses a file with an unknown time

The native, workload and regression records are rerun against the
remediated fixture build.

## Also in this tree (P05 push, C16)

C16's follow-up evidence is added for the P05 raise:
- native C16-N10 and C16-N11;
- the second integration as a loopback fixture mode;
- a late workspace-detail case in the App test;
- three C16 probes.

None of these changes C18's behaviour. The C16 regression run in this
chain exercises them.
