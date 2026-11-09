# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- The `guard` attribute: on a plain LEZ handler, `fn(&ReceiveInput, Message) -> Response` under `lee_core::actor_entrypoint!`, it puts the guard in front and leaves the program's code as it is. `roles(..)`, `config`, `seed`, and `genesis` for a genesis key.
- `Gated`, which a program's messages declare to the guard: `needs::<R>()` for a message that needs role `R`'s holder, checked before the handler, and `only_from_config()` for one that only the config account may send. `#[derive(Gated)]` with `#[only(R)]`, `#[only(A, B)]` and `#[from_config]`.
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

[Unreleased]: https://github.com/mmlado/lez-authority/compare/v0.1.0-alpha.1...HEAD
[0.1.0-alpha.1]: https://github.com/mmlado/lez-authority/releases/tag/v0.1.0-alpha.1
