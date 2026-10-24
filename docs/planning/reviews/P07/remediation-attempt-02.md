# P07 remediation of attempt-02

Codex attempt-02 scored 9/9/9/8/8. It resolved P07-F1/G1 (the keyboard
import) and raised one required fix and one evidence gap, both about the
production footprint.

## P07-R2-F1 / R2-G1: the footprint is measured on this tree

Attempt-02 cited `evidence/C24/footprint.json`. That record was measured in
C24's raise chain on C24's tree. This phase changes the shipped import view,
so it could not stand for the current production package, and the packet's
claim that it came from the same tree was no longer true.

- Code (`crates/xtask/src/budgets.rs`):
  - `carries_every_budget` holds for the cumulative chunk (C24) and for a
    phase containing it.
  - `required` and `status` use it. So `xtask footprint --phase P07` builds
    and measures the complete unsigned production app (the sealed Harness
    engine, `tauri.harness.conf.json`) and its DMG, and records them as
    `evidence/P07/footprint.json`.
  - The P07 native gate now requires P07's own `footprint.json` and
    `perf.json`, and every registered workload record, and evaluates them
    against the unchanged registered limits (budgets.json version 5). Before,
    it checked only the workloads its members measure.
  - The gate also checks that the measured production app is still the one on
    disk.
- Test: `the_phase_containing_the_cumulative_chunk_measures_every_budget_itself`.
  P07 and C24 carry every budget; P06, P08 and C23 do not.
- Chain: the footprint is built and measured after the workloads, from the
  same tree the fixture bundle was prepared from.
- Kept as they were: `evidence/C24/footprint.json` and
  `evidence/C24/precursor.json` are C24's historical records. The precursor
  is still C24's accepted 0.2.0 snapshot.

No product source changed in this remediation. Every driver, workload and
probe ran again on a bundle prepared from this tree.
