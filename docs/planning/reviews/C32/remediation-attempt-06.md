# C32 remediation after attempt-06 (9/8/9/8/8, fail)

| Finding | Remediation | Where |
|---|---|---|
| R1 later attempts erasing an earlier uncertainty | Every kept message is its own record (an id per attempt). An unconfirmed record is never removed or downgraded by a later attempt of the same text, nor by another conversation: it is settled only by an operator message its own conversation added after its baseline (each such message settles one record), or by the operator (Put it in the box, Discard). A not-sent record is kept once per text and is superseded by a later attempt. Tests: "a later refused retry never downgrades an earlier unconfirmed send", "identical unconfirmed sends in two conversations are settled each by its own conversation only", plus the refused-to-unknown and observation-before-receipt cases | `store.ts`, `harness.tsx` |
| R2 the registered fixture command named no target | The C32 core tests (proposal hand-offs, read and change decisions with the native confirmation, the MCP catalog read, a confirmation outliving its conversation) form the `ui_v2_harness` target registered in `crates/desktop-core/Cargo.toml`, moved out of `harness_tools`. The exact `checks.json` command `cargo test --locked -p desktop-core --features test-fixtures --test ui_v2_harness` runs as its own check (`fixture-command`) in `evidence/C32/runs/run-checks.sh`, its log retained | `tests/ui_v2_harness/main.rs`, `Cargo.toml`, `run-checks.sh` |

B1/E1 (the C29 live Vulture verification receipt) stay owner-gated (`evidence/C29/live-scope-request.md`).
