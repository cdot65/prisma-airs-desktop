# Academy embed

Source: `R2App` Academy section, board R2Academy. Chunk: C34.

The Academy learner view (react-query, react-router, Academy ShellFrame) is
hosted inside the new shell unchanged: the new sidebar replaces its outer
frame, its inner routing and tests stay intact. Gamification appears only
here. The wizard's Academy step is the only other Academy surface.

Tokens: Academy's `--academy-*` variables remain its own; within the embed
scope the shell aliases nothing. The embed receives the Rust-owned theme.

Tests: existing Academy presentation tests unchanged; one shell test that the
embed mounts under `academy` and receives theme changes.
