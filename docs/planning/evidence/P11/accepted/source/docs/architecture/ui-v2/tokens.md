# Tokens

The desktop app owns its appearance. A local token file
(`apps/desktop/src/styles/tokens.css`, created in C28) defines these names as
CSS custom properties under `--v2-*`, with the light values on `:root` and the
dark values under `[data-theme="dark"]`. Academy's `--academy-*` variables are
left intact for the embedded learner view; where a shared Academy component
renders inside the new shell, the Academy variable it reads is aliased to the
`--v2-*` value in the shell's scope only. No colour is written inline in a
component.

## Colour (from `L` and `D` in `prototype/R2App.dc.html` line 677–678)

| Token | Light | Dark | Use |
|---|---|---|---|
| bg | #f6f2e9 | #1d2021 | Page background (body gets this explicitly) |
| side | #ebe5d8 | #161919 | Sidebar and wizard rail |
| paper | #fbf8f1 | #282828 | Cards, dialogs, sheets |
| inset | #efe9dd | #32302f | Selected row, inset panels, explain track |
| rule | #e0d9ca | #3c3836 | Borders between regions, secondary buttons |
| ctl | #857d6e | #8a7f70 | Control borders (inputs, selects, toggles) — AA against bg and paper |
| ink | #14141a | #ebdbb2 | Primary text |
| soft | #4a463c | #d5c4a1 | Secondary text |
| faint | #635d50 | #bdae93 | Tertiary text, meta lines — still AA at 12px on bg |
| acc | #a33d15 | #fe8019 | Accent: primary buttons, focus ring, selected sidebar item |
| accInk | #ffffff | #1d2021 | Text on accent |
| accSoft | #f4dfd3 | #4a2f1a | Accent tint backgrounds |
| block | #b42318 | #ff6b57 | Verdict Blocked, Critical severity |
| blockBg | #fbe4e1 | #3d1f1c | Blocked background |
| flag | #8a5300 | #fabd2f | Verdict Flagged, Warning severity |
| flagBg | #faeed5 | #3a3019 | Flagged background |
| ok | #1f6f3a | #b8bb26 | Verdict Allowed, success |
| okBg | #def0e3 | #2b2f17 | Success background |
| sel / selInk | = acc / accInk | = acc / accInk | Selected tenant in the switcher |
| get | #245e8f | #83a598 | HTTP method and link colour in the API Client |

Verdicts and severities are never colour alone: every use pairs the colour with
an icon and a word (invariant I9).

## Spacing, type and size

| Token | Value | Use |
|---|---|---|
| page-margin-top | 32px | Every destination's top margin |
| page-margin-x | 44px | Every destination's side margin (wizard main: 34/44/24 add-mode, 32/44/28 first-run) |
| title | 26px / 700 | Destination and wizard step titles |
| card-pad | 18–20px | Card and dialog inner padding |
| gap | 10–12px | Between controls and cards; inner card gap 12px |
| text | 14px | Body |
| text-2 | 12px | Secondary text. 11px only for pills, eyebrows and mono badges |
| control-h | 32px | Inputs, selects, buttons |
| radius | 7px controls, 12px cards | |
| rail-w | 244px | Sidebar and add-tenant wizard rail (first-run wizard rail 320px) |
| reading-max | 860px | Reading width cap for prose and forms |
| focus | 2px solid acc, offset 2px; `data-inset-focus` buttons use offset −4px | Visible focus everywhere |
| window-min | 1180×720 | Rail collapses to icons below 1280 wide; no horizontal scroll anywhere |

## Theme switching

Theme remains Rust-owned (`setAppearance`). The shell sets `data-theme` on the
document root from the observed appearance and `prefers-color-scheme` for
"system". Tokens are the only place a theme value lives.
