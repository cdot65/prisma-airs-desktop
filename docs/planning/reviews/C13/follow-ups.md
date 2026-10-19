# C13 review follow-ups

C13 completed at 8/10 under D034 (0d105e0, Codex attempt-01 scored
9/8/9/8/8 with no blockers). These findings stay open until C13 is raised to
a passing review before the P04 phase sign-off.

## Attempt-01 (open)

- R1 (high): `statistics()` and `skill_statistics()` can turn invalid or
  contradictory statistics into successful zeros: an empty or error envelope
  is accepted, the quiet test ignores positive top-level totals and looks at
  bars already truncated to 64, and a missing unique-skills count becomes 0
  beside a positive vulnerability count. Plan: validate a recognised,
  error-free envelope and every relevant count before presentation limits;
  infer zero only from a validated, entirely quiet source; raw-response
  fixtures for each case must yield an invalid statistics source while the
  other sources stay usable.
- R2 (high): attack completeness is derived from requested offsets, not
  accumulated coverage, so repeated empty pages against `total_items=150`
  claim "all attacks read" and "no attacks". Plan: track rows actually
  received; empty or short pages with a positive remainder, later failures
  and the ten-page cap stay partial; complete-empty and 100/100/50 paging
  still complete.
- R3 (medium): `count_capped` is dropped after deriving `complete=false`,
  so a capped total renders as exact. Plan: carry total exactness through
  the projection and validator; a capped total renders as a lower bound, an
  exact total stays exact with sampled rows.
- E1 (T20): the registered model-summary workload is 100 Model Security
  scans and 100 skills; `measure_c13.py` served 12 skill scans. Plan: serve
  and assert 100 skill rows and re-measure timing and resident memory on the
  registered dataset, keeping the preregistered limits.
- Suggestion: cover an ordinary unknown label (`FUTURE_STATUS`); D039 and
  v1 map unknown labels to OTHER, `label()` shows "FUTURE STATUS".
- Probe: "Model Security sends the tenant header" survived because the SDK
  constructor forces that header off; replace it with a probe the suite can
  observe.
