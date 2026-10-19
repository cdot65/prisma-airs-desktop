# C14 review follow-ups

C14 completed at 8/10 under D034 (8d219b0). Codex attempt-02 scored
9/8/9/9/8 with no blockers, after attempt-01's R1–R5 and E1 were remediated
(`remediation-attempt-01.md`). These findings stay open until C14 is raised
to a passing review before the P04 phase sign-off.

## Attempt-02 (open)

- R5 (remaining, medium): body ownership is keyed by the literal reference
  string. On a case-insensitive volume `shared.body.txt` and
  `SHARED.body.txt` name one file but are separate owners, so a Save can
  overwrite another request's body. Plan: compare ownership by the
  filesystem identity of the resolved file (device and inode, or a
  case-folded key where the file is absent) and refuse ambiguous aliases;
  a test on the case-insensitive volume must return sharedBody with every
  project byte unchanged while independent bodies still save.
- Suggestion: the "leaving a document" React test only asserts an eventual
  Protect, so replacing the unmount flush survived. Plan: assert the Protect
  is sent on unmount before the debounce would have fired.
- Probe: "parent components are accepted in paths" (both guards removed)
  still survived. Plan: find which later check refuses `..` in the guarded
  test and either target the test at `relative_path` directly or record the
  remaining check as the guard.
