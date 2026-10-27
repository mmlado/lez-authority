# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
