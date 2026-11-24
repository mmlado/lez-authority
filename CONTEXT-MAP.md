# Context map

Three contexts share this repository, one for each crate. Each has its own glossary, and each role's builds on the engine's.

- **The engine**, lez-authority: [`CONTEXT.md`](CONTEXT.md), with its decisions in [`docs/adr/`](docs/adr/). The slot, the hop to the config and the guard, with no notion of any one role.
- **The admin**, lez-admin-authority: [`lez-admin-authority/CONTEXT.md`](lez-admin-authority/CONTEXT.md). The admin role on top of the engine. It has no ADR set of its own, the decisions it rests on are in [`docs/adr/`](docs/adr/).
- **The freeze**, lez-freeze-authority: [`lez-freeze-authority/CONTEXT.md`](lez-freeze-authority/CONTEXT.md), with its decisions in [`lez-freeze-authority/docs/adr/`](lez-freeze-authority/docs/adr/). The freeze role, which answers to the admin and uses both glossaries.

[`docs/adr/`](docs/adr/) also holds the decisions about the repository as a whole, such as lez-authority ADR-0007. Both sets number from 0001, so an ADR is named with its set: `lez-authority ADR-0004`, `lez-freeze-authority ADR-0003`, and `v0.2 ADR-0013` for the SPEL-based v0.2 line.
