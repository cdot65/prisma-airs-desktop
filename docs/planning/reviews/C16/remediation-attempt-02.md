# C16 remediation after attempt-02

Verdict attempt-02 (the P05 raise) scored architecture 9, behavior 9,
authority/security 9, validation 8 and overall 8. It had no required fixes
or blockers and one evidence gap. It found attempt-01's R1 and R2 closed
in the implementation.

## G1: the causal qualification record is current and complete

The raise's probe rerun aborted before any follow-up probe ran. The
"focus is not returned on close" probe anchored on
`opener.current?.focus()`, which C17 made ambiguous: the Gateway view now
also returns focus when a change closes (`closeChange`). The anchor now
includes the line before it, `setDetail(null)`, so the probe mutates only
the detail panel's `close`, which is what its guard
(`opens a detail with focus on its heading and returns focus on close`) covers.

The C16 probe set was then rerun in full with the hardened harness on this
tree (`docs/planning/evidence/C16/mutation-probes/`):

- `results.json` and `probes.log` are replaced by that run. Their `inputs`
  record the SHA-256 of every mutated source, as found in this tree.
- Every baseline passed three times; logs are in `baselines/`.
- All 33 mutations were caught: each made its named guard test fail.
- This includes the three follow-up probes declared in `follow-ups.md`:
  - `a detail keeps another integration's models`
  - `a late workspace detail is shown under another workspace`
  - `the workspace detail is never read`

  Each is credited only when its named App test fails.

The aborted run is kept as history in
`probes-aborted-raise-attempt-02.log`. The superseded summary from before
C17 is replaced, not merged: no result from it is carried over.
