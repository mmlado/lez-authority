# Domain Docs

How the engineering skills should consume this repo's domain documentation when exploring the codebase.

## Before exploring, read these

- **`CONTEXT-MAP.md`** at the repo root: it points at one `CONTEXT.md` per context, the engine, the admin and the freeze. Read each one relevant to the topic.
- **`docs/adr/`**: the decisions of the engine and of the repository as a whole. Read the ADRs that touch the area you're about to work in.
- **`lez-freeze-authority/docs/adr/`**: the freeze's own decisions. The admin has no ADR set of its own.

If any of these files don't exist, **proceed silently**. Don't flag their absence, and don't suggest creating them upfront. The `/domain-modeling` skill (reached via `/grill-with-docs` and `/improve-codebase-architecture`) creates them lazily when terms or decisions actually get resolved.

## File structure

```
/
├── CONTEXT-MAP.md
├── CONTEXT.md                         ← the engine
├── docs/adr/                          ← the engine and the repository
├── lez-admin-authority/
│   └── CONTEXT.md                     ← the admin
└── lez-freeze-authority/
    ├── CONTEXT.md                     ← the freeze
    └── docs/adr/                      ← the freeze
```

## Use the glossary's vocabulary

When your output names a domain concept (in an issue title, a refactor proposal, a hypothesis, a test name), use the term as defined in the context's `CONTEXT.md`. Don't drift to synonyms the glossary explicitly avoids.

If the concept you need isn't in the glossary yet, that's a signal: either you're inventing language the project doesn't use (reconsider) or there's a real gap (note it for `/domain-modeling`).

## Name an ADR with its set

Both ADR sets number from 0001, so every ADR reference names its set: `lez-authority ADR-0004` for `docs/adr/`, `lez-freeze-authority ADR-0003` for `lez-freeze-authority/docs/adr/`, and `v0.2 ADR-0013` for the SPEL-based v0.2 line.

## Flag ADR conflicts

If your output contradicts an existing ADR, surface it explicitly rather than silently overriding:

> _Contradicts lez-authority ADR-0004 (the guard in front of a plain handler), but worth reopening because…_
