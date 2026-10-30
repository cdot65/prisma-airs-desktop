# C44 parity notes — closure (no board)

C44 has no prototype board of its own: it removes the legacy shell and closes the phase. `parity/app/<destination>-{light,dark}.png` is the whole-app screenshot set `tools/native-acceptance/acceptance_c44.py` captures from every destination the catalogue lists, against the bundle in `native-prepare.json`; each screen's own board composite is under its chunk (C35–C43). The destinations without a board (Sessions & Reports, Import from v1, Settings) are the C11/C23/C02 views framed by the v2 shell, unchanged.

## What changed, and why

- **One presentation.** The core's snapshot always says `v2` with the v2 catalogue; an older store's persisted `legacy` is ignored; `setUiVariant` is accepted and ignored so older presentations and drivers are not refused. The Settings "Interface" switch, the legacy shell (`desktop-shell.tsx`, its palette and icons) and the legacy feature views no v2 screen imports are removed; the owner's uncommitted legacy files stay (legacy-script-map.md lists them).
- **The product destinations** (Runtime Security, AI Gateway, Red Team & Models in Go and the palette) open Administration on their product rather than the legacy pages.
- **The first run is the wizard**, so the native drivers' tenant setup walks it once (`v2_setup.py`) and adds the synthetic-endpoint tenant through Settings' tenant form.

## Owner's judgement

Recorded 2026-10-30: the owner directed the gates to proceed without a per-screen pause ("go go gog og o go gog o. stop stopping. go go go go go until we are 100% complete. stop stopping. go go go go go", "please continue my guy", "done. do it", "no matter what autonomy I give you, you keep stopping"). The screenshot set is posted for the record; that direction is the judgement this gate runs on; an objection reopens the chunk.
