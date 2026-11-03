# API Client and Test Cases

Source: `R2App` (`REQ`, `hist`, `send`, `rawJson`, `CASES`, `packCases`,
`caseRows` lines 1033–1037, `runScan`). Chunk: C33. Both are re-skins of the
existing workbench (C14–C15) and library (C12) modules to the layout rhythm.
No new remote capability is introduced.

## API Client

- Title row, request tabs (named requests per product), method/URL line with
  the `get` colour, Send (32px), response pane with status, duration and
  body; history list of immutable snapshots (I5) from the existing workbench
  history.
- After Send, a compact `role="status"` region announces "<request> · <code>
  · <duration>"; the JSON body is outside the live region.
- Generic requests never bypass AIRS mutation authority (existing C15 rule):
  a request that would change configuration is refused by the core as today.
- The "Show the calls" panel reuses this response renderer.

## Test Cases

- Title row, filter by product, search, paged table (name, product, input,
  last result as verdict colour + icon + word), pack-sourced cases carry
  their pack name.
- **Runtime-only execution boundary** (prototype lines 1035–1037, parity
  matrix). A Runtime Security case offers "Open in Playground": it sets the
  Playground's prompt, direction and profile and the operator runs the scan
  there; the result is stored for the case as a local Playground run. A case
  for any other product offers "Inspect" only: it opens the specification
  and the recorded result, states which product runs it, and never
  dispatches a scan or claims execution. Red Team cases say "hand the spec
  to a scan"; no scan is started from this screen.
- Starter-pack cases selected in the wizard are written at onboarding
  completion as library entries with `Not run` results.

## Fixtures and tests

- Boards R2Api, R2Cases. Tests V2-R05, V2-T05; V2-T14 status region updates
  after Send with the body outside the live region; V2-T15 non-Runtime rows
  render Inspect, never Open in Playground, and clicking Inspect dispatches
  no scan intent (asserted on the mock runtime); V2-T16 paging and filter.
