# P03 phase review follow-ups

## Attempt-01

- R1 (medium, cross-feature context isolation): a Library case loaded into
  the Playground, and a pending Save this case, outlived their context. The
  shell now binds the handoff to the context it was loaded in and checks it
  while rendering (`prefill` is shown only when `loaded.context` equals the
  active context; it is also dropped when the context changes, so returning
  never revives it). `RuntimeView` is keyed by context like Sessions, Library
  and Administration: a tenant switch or credential replacement mounts a
  fresh form, case name and save status, and a late save admission for the
  old instance has nowhere to land; its effect-based reset was removed.
  Regression (production App/shell): "retires a loaded case and a pending
  save with their context" loads A's case, holds A's SaveCase admission,
  switches to tenant B with the Playground mounted, inspects every node
  mounted during the switch (none carries A's prompt, name or banner),
  releases the admission and delivers a Saved notice under B (no status
  shown), returns to A (nothing revived), then replaces A's credential
  context after a fresh same-context load. C12 probes "the Playground keeps
  its instance across contexts" and "a loaded case renders under another
  context until an effect clears it" are killed by it.
