# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

Up to 0.2.0 this file covers lez-authority only. From 0.3.0 it covers the three crates of this repository, which share one version, with a section for each crate that changed. lez-admin-authority's releases up to 0.2.0 are in [its own changelog](lez-admin-authority/CHANGELOG.md), and lez-freeze-authority's first release is 0.3.0.

## [Unreleased]

lez-admin-authority and lez-freeze-authority live in this repository, beside lez-authority, and the three crates share one version and one tag ([lez-authority ADR-0007](docs/adr/0007-one-repository-at-one-version.md)).

### lez-admin-authority

#### Changed

- The crate lives in `lez-admin-authority/` of this repository, and a program depends on it through this repository's URL and tag.
- lez-authority comes in by path, at the same version, in place of its `v0.2.0` tag.
- The sample's packages are `admin-sample`, `admin-sample-core`, `admin-sample-minimal`, `admin-sample-methods` and `admin-sample-runner`. The runner is a workspace of its own, with its own lock and audit policy, and the methods crate's lock is tracked.

## [0.2.0] - 2026-11-11

### Changed

- `lee_core` is pinned to logos-execution-zone `v0.4.0-rc3`, by tag, in place of `dev` at `7e3f7f7`. A consumer pins the same tag, spelt the same way. The guard, the wire formats and the refusal codes are unchanged.
- The README and `docs/routing.md` describe the rc3 wallet: it runs a transaction before sending it, does not send one that reverts unless the client allows it, and declares the gas it measured.

## [0.1.0] - 2026-11-10

### Added

- The `guard` attribute: on a plain LEZ handler, `fn(&ReceiveInput, Message) -> Response` under `lee_core::actor_entrypoint!`, it puts the guard in front and leaves the program's code as it is. `roles(..)`, `config`, `seed`, and `genesis` for a genesis key.
- `Gated`, which a program's messages declare to the guard: `needs::<R>()` for a message that needs role `R`'s holder, checked before the handler, and `only_from_config()` for one that only the config account may send. `#[derive(Gated)]` with `#[only(R)]`, one role per message, and `#[from_config]`.
- `#[derive(HasSlot)]` with `#[slot(R)]` on the config's field.
- `lez-authority-macros`, a crate of this workspace re-exported here, for the attribute and the derives.
- `Wire`, `LibraryMessage` and `LIBRARY_TAG`: the library's messages travel behind the first byte 255, so the program's enum hosts none of them. `Wire::op::<R>(op)` builds a client's operation.
- `Policy<C>`: who may install, nominate and renounce a role, and what renounce leaves. The defaults are the rules of a role that answers to nobody. A role that answers to another overrides them and reads the other role's slot through the config.
- `Roles`, implemented for `(A,)` and `(A, B)`: a program names its roles as a tuple, and the first one's `Init` creates the config.
- `Refusal`, a refusal with the role it names, and `Ctx::check` for policies.
- E135 `LibrarySend`: wherever the handler runs, its response may not send the config account a library message.
- `examples/owner_by_hand.rs`, the README's program without the macros, `tests/macros.rs`, and ADRs 0003 and 0004.

### Changed

- `guard` and `try_guard` take the program's roles, `(A,)` for one, a `Wire<M>`, and a plain handler. `try_guard` returns a `Refusal`.
- A message that needs no holder runs wherever it is sent, as in a program without the guard. Only operations and messages that need a holder go through the sender's seat.
- The hub is the handler's own response: it sends a `#[from_config]` message to another actor of the program, which runs it only when the config account sent it.
- `Ctx` is what a `Policy` sees: the caller and the init gate.
- E123 covers `#[from_config]` messages, E126 the handler's write, E108 an operation naming a role the program does not have.
- The wire format of the library's messages: they start with 255 and name their role. The program's own messages keep their encoding.

### Removed

- `Hosts`, `Hub`, `guard_with_targets`, `try_guard_with_targets`, and the handler's context: `Ctx::require`, `install`, `nominate`, `accept`, `renounce` and `send`.
- E132 `SendNeedsHolder` and E133 `TargetDoesNotDecode`, retired with them.

## [0.1.0-alpha.1] - 2026-10-27

### Added

- `AuthoritySlot`: unset, held by a `Holder` with its `via` and at most one pending nominee, or renounced for good.
- `Caller`, the sender of a hopped message as the guard saw it. Only the guard builds one.
- `Role` and `HasSlot`, the two traits a program implements to say what a slot is and where it lives.
- `Hosts`, the message enum's hosting of the library's variants: the operations `Op`, the hop `Forwarded`, and the hub `Hub` for programs with targets.
- `Op`: `Init`, `Nominate`, `Accept` with the seed proof for a PDA nominee, and `Renounce`.
- `InitGate`: the deployer, the program's own account, or a genesis key.
- `Ctx`: `caller`, `require`, `install`, `nominate`, `accept`, `renounce`, and `send` to a target after the holder was verified.
- `guard` and `try_guard` for programs whose state lives at the config account, `guard_with_targets` and `try_guard_with_targets` for programs with targets.
- `AuthorityError` with stable codes, `code`, `text` and `describe`.
- `config_account` and the `seed` const fn.
- The README with the model, the refusal table and the security properties, `docs/routing.md` with every hop and its checks, and the README's program as `examples/owner.rs`.

[Unreleased]: https://github.com/mmlado/lez-authority/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/mmlado/lez-authority/releases/tag/v0.2.0
[0.1.0]: https://github.com/mmlado/lez-authority/releases/tag/v0.1.0
[0.1.0-alpha.1]: https://github.com/mmlado/lez-authority/releases/tag/v0.1.0-alpha.1
