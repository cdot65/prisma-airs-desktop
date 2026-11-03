# UI v2 "Refined Native" — design extraction

This folder is the source of truth for the second-generation desktop
presentation: the shell, first-run wizard, Inbox and reads, reviews,
Administration, Harness, API Client, Test Cases and the Academy embed, as
reviewed on the Claude Design canvas and frozen at version 134 (2026-10-26).

Record: decision D058 in [decisions.md](../../planning/decisions.md). Phase:
P09, chunks C27–C34 ([chunk index](../../planning/chunks/README.md)).

| File | Role |
|---|---|
| [DESIGN-V2.md](DESIGN-V2.md) | Scope, frozen reference, layout rhythm, destinations, error presentation, accessibility additions, budgets, known gaps |
| [tokens.md](tokens.md) | The token table for both themes and the mapping onto Academy variables |
| [state-ownership.md](state-ownership.md) | Every prototype state field assigned to Rust-owned/observed or UI-transient |
| [invariants.md](invariants.md) | The twelve normative rules with their acceptance test ids |
| [screens/](screens/) | One interaction contract per screen family, plus [boards.md](screens/boards.md) (fixture table) and [administration-inventory.md](screens/administration-inventory.md) (every product, tab and action) |
| [provenance.md](provenance.md) | Intentional deviations from the prototype, with reasons |
| [prototype/](prototype/) | Frozen copies of the six reviewed R2 files and their SHA-256 manifest. Immutable inputs; never edited |

The canvas (https://claude.ai/artifact/2SEXT1XMHdZkxHvdw8oaYL, page "Refined
Native v2") is the design record only. After version 134 it is not changed;
further exploration happens on a new page. The prototype's data is invented
and is never shipped; it is used only to write fixture content.
