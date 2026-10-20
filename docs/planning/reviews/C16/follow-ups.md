# C16 follow-ups for the P05 push to 9/10 (D034)

C16 completed at 8/10 under D034. Codex attempt-01 scored 9/8/9/8/8 with no
blockers and two required fixes. Both are remediated in C17, which edits the
same Gateway view, and are re-reviewed with it and at the P05 push.

- R1, integration models bound to the integration shown: DetailPanel keeps
  its models across a change of row and commits a model reply without
  checking which integration is current, so A's models (or a late A reply)
  can appear under B. The fix keys model state to the integration and drops
  replies for any other. A regression test drives both transitions (cached
  and pending A, then B) through App/DesktopClient.
- R2, the chosen workspace's detail readable from the UI: no control reads
  `Detail { resource: workspaces }`, so a workspace's defaults and embedded
  usage/rate limits are unreachable. The fix adds a read-only, keyboard-reachable
  "Workspace details" view of the selected workspace with
  loading/denied/error states, retired on selection and context changes, with
  App/DesktopClient tests and native keyboard/focus evidence.
