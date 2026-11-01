# Worf check attempt-03 — retained failure and fixture correction

The fresh run passed 17/18 checks on unchanged inputs. `core-fixtures` failed
all three retained runs in `imports_rollback`; all other checks passed. Original
logs, hashes and pre-correction sources remain immutable. The first targeted
correction passed 22/23 and still failed the exhaustive interruption test; the
second targeted run passed all 23, with every test and assertion retained.

The harness had omitted the production `Core::shutdown()` exit hook, polled
temporary receipt JSON before its durable copies/document binding completed,
and guessed that 400 ms reached every configured interruption point. It now
uses the real exit hook and confirmed-durable document, and waits for the armed
fixture cut to fire. A read-only observer is appended under `test-fixtures`
only; no production definition, IPC authority or live operation is changed.

The complete first 19-driver native generation, including its exact executable,
is preserved under `native-history/Worf-generation-01-complete-20261101`.
Generation 02 is rebuilt and must rerun every mapped driver on its new bytes,
then fresh checks and actual independent reviews. No old pass is transferred.
The fresh before/after build seal is `preparations/worf-02`; its exact source
diff and build log are retained. Current audits allow no source exemptions.

No real credential, alpha data or product endpoint was used. L19's three reads
remain charged with zero allowance. This correction does not complete P09.
