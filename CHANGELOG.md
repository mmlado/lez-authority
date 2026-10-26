# Changelog

All notable changes to this project are documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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
