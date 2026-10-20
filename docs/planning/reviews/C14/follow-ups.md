# C14 follow-ups for the P04 push to 9/10 (D034)

C14 completed at 8/10 under D034 (8d219b0). Codex attempt-02 scored
9/8/9/9/8 with no blockers, after attempt-01's R1–R5 and E1 were remediated
(`remediation-attempt-01.md`). The remaining findings are remediated in the
tree re-reviewed as C14 at the P04 push; D040 and DESIGN.md's save row
record the rule.

- R5 (remaining, medium), body ownership by the file reached:
  - `document::shared` no longer compares the literal reference. A body is
    written only when the file it reaches is owned by no one else: not
    another request file's body (duplicates and invalid ones included),
    and not a project file (any request file, this one included,
    `collection.toml`, an environment, a schema).
  - Names are compared by the file itself (device and inode of the
    resolved path) or, for a file not yet written, by the case-folded
    name. Case and Unicode-normalization aliases on the Mac's volumes are
    therefore refused as `sharedBody`.
  - Tests:
    - The production-interface
      `a_body_is_owned_by_the_file_it_reaches_not_its_spelling` runs on the
      case- and normalization-insensitive temporary volume. A different
      case, a decomposed Unicode spelling, another request's metadata file
      and `collection.toml` each return `sharedBody` with every project
      byte unchanged, and an independent body still saves.
    - The unit test
      `a_body_not_yet_written_is_owned_by_its_case_folded_name` covers a
      missing file reserved by an invalid request.
- Suggestion, leaving a document: the React test is now "protects pending
  edits when leaving the document, before the pause would". It requires the
  Protect within a quarter of the typing pause after the editor leaves, so
  the pause's own timer cannot satisfy it; the "leaving a document drops
  pending edits" probe is caught.
- Probe, parent components: with `creating` set, the hidden-name rule
  refused `..` too, so removing both parent guards survived. The guarded
  test now also checks `..` and `.` with `creating` off (`../a`, `a/../b`,
  `a/..`, `./a`, `a/./b`), and the probe is caught.
- Probe anchors: "Save sends the loaded fields" names the Save snapshot
  uniquely (C15 added a Send snapshot). "Duplicate requests lose their body
  ownership" targets the new owner filter. Three probes cover identity,
  case and project-file aliases.
