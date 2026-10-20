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

## Attempt-04 (P04 push re-review; 9/8/9/8/8)

- R1, an unchanged Save supersedes an older draft: when the submitted
  snapshot equals the saved version, Save removes the document's protected
  draft and its recovered mark, as Protect already did. That removal is
  stored before the Save is acknowledged, so a quit after the reply never
  recovers the older text. Tests:
  - the production-interface
    `saving_the_saved_snapshot_drops_an_older_protected_draft`: Protect(B),
    Save(A) is `unchanged` with fields A and no draft; after a restart and
    reopen the fields are A, nothing is recovered, and every project byte
    is unchanged.
  - React: "saves a reverted document before the pause, so the older
    protected draft is superseded". It sends A, shows "No changes to save",
    and no protection follows.
- Suggestion, bounded enumeration: discovery reads a directory no further
  than the remaining entry budget plus one, so a huge directory costs no
  more than the limit it trips. An unreadable directory entry makes the
  catalog incomplete (`invalid`) instead of being dropped. Test:
  `a_directory_past_the_entry_budget_makes_the_catalog_incomplete`.
- Suggestion, evidence: C07's offline storage manifest is now written with
  C07's native records (`native-C07/storage-manifest.json`) when it runs as
  a regression under a later subject. The misplaced top-level copies in C14
  and C15 are removed.
- Three new probes cover these.

## Attempt-05 (P04 push re-review; 9/8/9/8/8)

- R6, a JSON body is its request's: attempt-03's ownership guard counted
  every `.json`/YAML schema candidate as a project file. A request's own
  `.body.json` sidecar, including the built-in catalogue's 53 JSON bodies,
  therefore could not be saved. Schemas are now owned by their role:
  - a schema a request associates (`schema = "…"`, recorded per request
    file in `Catalog.schema_links`) is never a body's to rewrite;
  - a standalone schema (a candidate no request names as its body,
    compared by case-folded name or device and inode) is never a new
    body;
  - a candidate some request names as its body is that body, owned by the
    requests naming it, and checked by the body-ownership rule.

  Body names and identities are computed once, so the check stays linear
  in the inventory. Tests:
  - the production-interface
    `a_json_body_saves_as_its_requests_while_an_associated_schema_stays_owned`:
    an exclusively owned `items/post.body.json` saves, every other project
    byte (its metadata included) is unchanged, and it reopens after a
    restart. A body that is another request's associated schema is
    `sharedBody`.
  - `the_built_in_collection_is_private_generic_and_keeps_edits` now saves
    a catalogue request's JSON sidecar, keeps its metadata byte for byte,
    and reopens it.
  - the unit test
    `a_json_body_is_its_requests_and_a_schema_is_owned_by_its_role`
    covers standalone schemas by any spelling.
  - The alias refusals in
    `a_body_is_owned_by_the_file_it_reaches_not_its_spelling` are
    unchanged.
- Three new probes; "a body may be another project file" is re-anchored.
