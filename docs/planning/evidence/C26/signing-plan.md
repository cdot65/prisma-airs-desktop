# C26 signing plan (preparation, not C26 evidence)

C26 is not started. This records a signing trial on a copy of the P07
production build and the signing design that resulted. Nothing here signs,
reseals or changes a committed input.

## Trial on 2026-10-25 (a copy of the P07 production app)

- **Mach-O inventory.** Two arm64 executables, both ad-hoc signed by the
  linker: `Contents/MacOS/prisma-airs-desktop-v2` and
  `Contents/Resources/harness/airs-harness`.
- **Entitlements.** None. The Tauri configuration declares none, and the
  engine ran `--version` ("airs 0.1.7-desktop.2") under the hardened runtime
  without any.
- **Signing.** "Developer ID Application: Calvin Remsburg (G5QLZ5A8TA)",
  `--options runtime --timestamp`, the engine first, then the executable,
  then the app.
  - `codesign --verify --deep --strict` passed.
  - `spctl` before notarization reported "Unnotarized Developer ID", as
    expected.
- **Notarization.** The owner's Keychain profile `prisma-airs-desktop`
  (verified with a history read) submitted
  `6991ee77-7de1-434c-8e12-2e1eacbe425d`: **Accepted**, with no issues in
  Apple's log.
- **Stapling.** Stapled the DMG and the app; `stapler validate` passed, and
  `spctl` then accepted both as "Notarized Developer ID".
- **Launch.** The signed app started with a scratch HOME and was still
  running after 10 s.

## The finding: signing changes the engine's pinned bytes

The app checks the engine's SHA-256 against `binarySha256`, compiled from
`resources/harness/pin.json`, before it starts it
(`crates/desktop-core/src/harness/engine.rs`, `verify`). Signing rewrites
the engine:

| Engine | SHA-256 |
|---|---|
| Unsigned (pinned) | `f8e49cee…` |
| Signed in the trial | `2adce4c1…` |

A release that signed the engine after the build would therefore report
Harness unavailable.

## Design: sign the engine once, at sealing time

1. `cargo run -p xtask -- harness-sign --identity "<Developer ID>" --out <dir>`:
   - unpacks and verifies the sealed engine (`harness::unpack`);
   - refuses an engine that is not the pinned unsigned one, and a staging
     folder under `resources/`;
   - signs a copy (hardened runtime, secure timestamp) and verifies it;
   - writes `<dir>/airs-harness` and `<dir>/signing.json` (identity, team,
     unsigned and signed hashes).
2. `python3 tools/harness/seal.py <v1 archive> <repo> --signed-engine <dir>`
   reseals as before, after its usual v1 provenance checks. It uses the
   signed bytes only when the record names the pinned unsigned engine as its
   source and the file has the recorded signed hash.
   - The signed hash becomes `binarySha256`, so it is in MANIFEST.json and in
     the compiled pin.
   - `pin.json` keeps a `signing` block, including `unsignedBinarySha256`.
   - A Developer ID signature carries a secure timestamp, so a signed reseal
     reproduces from `<dir>`, not from the v1 archive alone.
3. The reseal and the new pin are reviewed C26 inputs. The app is then built
   with the signed engine bundled.
4. `cargo run -p xtask -- release-sign --identity "<Developer ID>" --app <app> --dmg <dmg> --execute --notarize`.
   It refuses before signing anything when the bundled engine is not the
   compiled pin or its signature does not verify. It never signs the engine
   again. Then it runs these steps in order:
   1. sign the main executable, then the app;
   2. zip the app with `ditto`, notarize it
      (`notarytool submit --keychain-profile prisma-airs-desktop --wait`) and
      staple it, so the installed app passes an offline assessment;
   3. build the DMG from the stapled app, sign it, notarize it and staple it;
   4. assess the app with `spctl -t exec` and the DMG with `spctl -t open`
      (primary signature), and run `stapler validate`.

   It writes `evidence/C26/release-signing.json` with each step's argv and the
   app-tree, executable, engine and DMG hashes before signing, after signing
   and after stapling. Without `--execute` it prints the plan only.

## Verified without the real identity

- **Unit tests** (`crates/xtask/src/signing.rs`, ad-hoc identity in a temp
  dir):
  - argv: timestamped for a real identity, `--timestamp=none` ad-hoc;
  - plan order, no step that touches the engine, and the DMG signed without
    the runtime flag;
  - the engine signed once from its pinned unsigned bytes, with the source
    untouched;
  - release signing that keeps the pinned engine's bytes and refuses another
    engine before signing anything.
- **End-to-end ad-hoc check** in a scratch folder:
  - `harness-sign --identity -` on the real sealed engine;
  - `seal.py --signed-engine` into a scratch repo, where the pin, MANIFEST
    inventory and resources digest all named the signed engine;
  - the committed `pin.json` and archive were unchanged byte for byte.

## Left for C26

- Run steps 1–4 with the Developer ID identity on the frozen post-C25 tree.
- Commit the reviewed reseal and pin.
- Run the installed-artifact checks (F01–F09) in a fresh non-admin macOS
  account.
- The trial's DMG came from `hdiutil create`, which warned that it is
  deprecated. Decide between that and Tauri's own DMG bundle (whose DMG
  `release-sign` would then sign as the last bundle step) before C26.
