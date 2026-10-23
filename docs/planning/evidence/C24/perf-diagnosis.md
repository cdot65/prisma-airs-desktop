# C24 cold-start diagnosis (budgets.json version 4)

C24's re-measurement of the core budgets on its bundle failed one of them
under budgets.json version 3: cold-usable-shell p95 2608.9 ms against
2000 ms, every sample between 2503 and 2616 ms
(`perf-v3-over-budget-20261023.json`). Warm launches (p95 about 350 ms),
input-to-paint, idle memory and scan memory passed. C03 and P01 had measured
cold p95 614 and 609 ms under the same method. This record is how the cause
was found. Every run was on the budgets' host (Mac17,3, macOS 27.0.1
(26A434)) on 2026-10-23, against the C24 prepared fixture bundle unless
stated.

## Findings

1. **The app's own start is not slower.** A fresh copy's first launch took
   2489-2743 ms; the same copy launched again with no data root (and its
   WebKit data removed) took 380-428 ms (3 copies).
2. **macOS's malware scan takes the time.** Summed CPU time of every
   process before and after one first launch: XprotectService +2.32 s,
   the app +0.19 s, nothing else above 0.35 s.
3. **Not an XProtect update.** XProtect was updated on 2026-10-22
   (5368, payloads 170), after C03 and P01. The P01 tree (80d6c513) was
   rebuilt with `xtask native-prepare --chunk C03` (16.02 MiB bundle, as
   P01 recorded) and measured alternately with C24, four fresh copies each:

   | Bundle | First launch of a fresh copy | XprotectService CPU | Same copy again |
   |---|---|---|---|
   | P01 | 709-1006 ms | +0.09-0.10 s | 339-371 ms |
   | C24 | 2569-2913 ms | +2.30-2.36 s | 372-395 ms |

4. **The bundle, not the executable.** Executables swapped between the two
   bundles (re-signed ad hoc), two runs each:
   - C24 bundle with P01's executable: 2331-2429 ms, XProtect +2.17-2.20 s;
   - P01 bundle with C24's executable: 735-804 ms, XProtect +0.19-0.20 s.
5. **The Harness engine.** The C24 fixture bundle carries the sealed Harness
   engine since C19 (D046): `Contents/Resources/harness/airs-harness`, a
   321 MB Mach-O executable, with its notices. The C24 bundle with
   `harness/` removed launched fresh in 684-715 ms with XProtect +0.18-0.20 s
   (3 copies).
6. **The cost follows size, not content.** C24's own sections (code
   20.4 MB, constants 6.8 MB, strings, data constants) embedded as-is in a
   minimal ad-hoc signed executable cost XProtect 0.01-0.13 s each, the same
   as 20 MB of random bytes (0.12 s): about 6 ms per MB, which the engine's
   321 MB (194 MB of code) turns into about 2 s.

## Consequence and change

The version 3 method spawns the executable from a fresh copy of the bundle
at a new path. Since the engine was bundled, that times macOS's one-time
assessment of a never-seen bundle, which happens once per installed copy, of
a resource the app does not open at launch, rather than the app's cold
start. budgets.json version 4 keeps the limit, statistic and samples; each
fresh copy is launched once and quit, its data root and WebKit data are
removed again, and its second launch is timed. The first launch of every
copy is still measured and reported (`details.cold[].first_launch_of_copy_ms`
in `perf.json`), so the first-launch cost an operator meets once after
installing stays visible. The change needs owner or independent reviewer
acknowledgement under the budget governance; C24's review is asked for it.

## Open for C26

The production bundle that `xtask footprint` measures does not carry the
engine (only the fixture build adds `tauri.harness.conf.json`; D046 leaves
signed release packaging to C26). The release bundle C26 builds will. With
the engine, that bundle is about 360 MB and its DMG holds the engine's
113 MB compressed archive or more, beyond the registered app-bundle
(150 MiB) and DMG (100 MiB) budgets, and its first launch after installing
carries the assessment above. C24 records this; how the engine ships against
those budgets is C26's to decide with the owner.
