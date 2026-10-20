# C13 follow-ups for the P04 push to 9/10 (D034)

C13 completed at 8/10 under D034 (0d105e0, Codex attempt-01 scored
9/8/9/8/8 with no blockers). Each finding below is remediated in the tree
re-reviewed as C13 at the P04 push; D039 and DESIGN.md's C13 section
record the tightened rules.

- R1 (high), statistics: `red_team::statistics` and `skill_statistics`
  first require a recognised reply. A reply reporting a failure (`success:
  false`, or any `error` or `errors` field, as v1's `reportsFailure`) or
  carrying none of its known fields is invalid. A breakdown has at most 64
  entries (more is invalid, as v1). Whether a missing total may be zero is
  decided from every reported count before anything is limited: both
  totals, every breakdown entry, every rated target. AgentGuard's groups
  must be objects, and a missing count is zero only with no top
  vulnerability and every reported count zero. Tests:
  - `statistics_need_a_recognised_error_free_reply`
  - `missing_totals_are_zero_only_when_every_count_is_zero`
  - the production-interface `invalid_statistics_fail_alone_and_the_other_sources_stay_usable`:
    raw replies `{}`, message-only, an error envelope, `success: false`, a
    positive partner total, 65 entries, an unknown label above zero.
    Statistics are `invalidResponse` while the scan list, models and skills
    stay usable.
- R2 (high), attack coverage:
  - `ManagementRequest::Attacks` carries `received`, the rows received on
    the pages before it. It is admitted only up to `100 · page`, and the
    core judges completeness from it, not from the page offset.
  - The view stops on a short page. It says "the service returned fewer
    than it reports" and shows "no attacks" only for a complete read; an
    incomplete empty read says the results could not be read in full.
  - Tests:
    - `attack_coverage_counts_rows_received` (100/100/50 completes;
      repeated empty pages against 150 never do; an empty report
      completes)
    - `attack_coverage_counts_rows_received_and_capped_totals_are_bounds`
      (production interface)
    - UI: "stops on a short page and never calls an unread remainder 'no
      attacks'", and "stops reading attack results once the total is
      covered or a read fails", which asserts the `received` sequence
      0/100/200.
- R3 (medium), capped totals: every list (Red Teaming scans, model scans,
  skill scans, attack pages) carries `capped`. The contract validator
  requires it and refuses a capped list called complete, and the view
  renders a capped total as "≥ N" in tiles, coverage lines and the attack
  summary. An uncapped total stays exact with sampled rows. A total below a
  page's own rows, or a non-boolean `count_capped`, is invalid (v1).
  Tests: UI "shows a capped total as a lower bound"; the contract test
  "rejects a total without its exactness, or a capped total called
  complete"; `list_pages_follow_v1s_shape_rules`.
- E1 (T20): `mgmt_server.py` serves 100 AgentGuard skill scans
  (`total_items` 100). `measure_c13.py` asserts "Skill scans · 100 read ·
  complete" before timing and RSS, and C13-N04 asserts 100 pseudonyms. The
  registered workload is re-measured on this chunk's prepared bundle with
  the preregistered limits unchanged (`workload-perf.json`).
- Suggestion, labels: breakdown labels now follow v1's per-breakdown lists
  (scan statuses, risk ratings, target types). Any other label, including
  an ordinary `FUTURE_STATUS`, becomes OTHER and still counts as activity.
  Test: `known_labels_are_shown_in_words_and_any_other_is_other`.
- Probe: the tenant-header probe is replaced. Each SDK constructor fixes
  that header (on for AgentGuard, off for Model Security), so no edit in
  this module can change it. The new probe instead sends the Model
  Security read to the AgentGuard plane, which the suite observes. 17 new
  probes cover R1–R3; the probes whose anchors moved are retargeted.

## Attempt-02 (P04 push re-review; 9/8/9/8/8)

- R1 (remaining), nested rated targets: `rated_targets` now validates
  every rating's `targets_by_type` before quietness is decided. Each must
  be an array of at most 64 objects, each with a whole, non-negative
  `count` (v1). A negative, fractional or missing count, a non-array group
  or 65 entries makes the statistics invalid, and valid all-zero groups
  stay quiet. Tests:
  - `rated_targets_are_validated_before_a_source_counts_as_quiet`
  - two raw cases added to the production-interface
    `invalid_statistics_fail_alone_and_the_other_sources_stay_usable`
- R4, pagination: a list page needs its pagination object (v1; the total
  may be omitted), and a present `count_capped` must be a boolean (null
  included). Such a page is invalid, so it never establishes completeness
  or proves "no scans". Tests:
  - `list_pages_follow_v1s_shape_rules`
  - the 404 confirmation path in
    `absent_statistics_mean_no_scans_only_when_an_empty_list_proves_it`
    (malformed confirmations fail the read with the absence, never "no
    scans")
  - malformed attack and model pages in
    `attack_coverage_counts_rows_received_and_capped_totals_are_bounds`
- Four new probes cover both; the quiet-source probe is re-anchored.
- Same class, found while remediating: every list row must be an object,
  and so must a skill scan's `summary` (v1). A bare number no longer reads
  as an unrated attack or an unreported count (`list_pages_follow_v1s_shape_rules`).
