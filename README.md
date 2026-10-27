# lez-authority

[![CI](https://github.com/mmlado/lez-authority/actions/workflows/ci.yml/badge.svg)](https://github.com/mmlado/lez-authority/actions/workflows/ci.yml)
[![Audit](https://github.com/mmlado/lez-authority/actions/workflows/audit.yml/badge.svg)](https://github.com/mmlado/lez-authority/actions/workflows/audit.yml)
[![Version](https://img.shields.io/github/v/tag/mmlado/lez-authority?include_prereleases&sort=semver&label=version)](https://github.com/mmlado/lez-authority/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.98.1-93450a?logo=rust)](rust-toolchain.toml)

Authority primitives for LEZ actor programs (LEZ 0.4): a slot per role inside a program's config, the hop that carries a sender's authority from their own actor to the config, and the guard that runs the program's own handler behind both. Policy-free: no notion of admin or freeze lives here. No SPEL dependency.

Status: alpha, `v0.1.0-alpha.1`. The API and the wire formats may still change before 0.1.0. Pinned to logos-execution-zone `dev` at 7e3f7f7 (2026-10-24), the actor model of PR #935. The v0.2.4 line lives in [spel-authority](https://github.com/mmlado/spel-authority) and stays as it is. First consumer: [lez-admin-authority](https://github.com/mmlado/lez-admin-authority), which names the admin role.

See [CONTEXT.md](CONTEXT.md) for the glossary, [docs/routing.md](docs/routing.md) for every hop with its checks, and [docs/adr](docs/adr) for the two design decisions.

## The problem

On LEZ 0.4 a program is one handler that runs at an actor, the pair of an account and the program. The runtime hands the handler the message, the actor that sent it (`from`), and one bit about the receiving account: whether it authorized the transaction (`is_authorized`). It says nothing about any other signer. So a program that keeps its state on one well-known account cannot ask, at that account, whether the admin signed. The admin's signature is visible only at the admin's own actor.

## The model

A message that needs the holder of a role goes to the sender's own actor under the program first, the seat. The seat checks `is_authorized` and forwards the message to the config account, naming the program that reached the seat. The config trusts the forward because only an actor of the same program can produce one, and every actor of the program runs the same code: the guard.

```text
               seat (sender, P)              config (C, P)                     target (T, P)
client ──────► is_authorized?     hop       from an actor of P?      hub       from == (C, P) exactly?
               via = the program ─────────► caller = from.account ──────────► target handler
               that reached me              slot.check(caller)
                                            handler(ctx, config, message)
                                            slot changed only through ctx?
```

Two hops for a gated message, one for an open message sent straight to the config, three when the holder reaches another actor of the program through the config, the hub. Identity comes from the engine's `from` at every hop, never from a field in the message.

A holder is an account together with the program it acts through, its `via`: none for a key that signs at top level, the owning program for a PDA. The guard accepts a caller only when both match ([ADR-0001](docs/adr/0001-per-holder-via.md)).

## Adding as a dependency

```toml
[dependencies]
borsh = { version = "1", features = ["derive"] }
lee_core = { git = "https://github.com/logos-blockchain/logos-execution-zone", rev = "7e3f7f765371c4110a1f7f37baf520cd253fb339" }
lez-authority = { git = "https://github.com/mmlado/lez-authority", tag = "v0.1.0-alpha.1" }
```

A program that only needs an admin depends on [lez-admin-authority](https://github.com/mmlado/lez-admin-authority) instead, which brings this crate in. The `lee_core` source must match the one this crate pins, see [Versioning and stability](#versioning-and-stability).

## What a program writes

The program keeps its own message enum and its own handler. It hosts the library's variants in the enum so that they travel on the same wire, puts a slot in its config, and wraps the handler in the guard ([ADR-0002](docs/adr/0002-guard-around-a-plain-handler.md)). A downstream crate names the role, as lez-admin-authority does for `Admin`. The whole of [`examples/owner.rs`](examples/owner.rs):

```rust
use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::program::{PdaSeed, ReceiveInput, Response};
use lez_authority::{AuthoritySlot, Ctx, Forwarded, HasSlot, Hosts, InitGate, Op, Role};

/// The role this program has one holder of.
pub struct Owner;

impl Role for Owner {
    const NAME: &'static str = "owner";
}

const CONFIG_SEED: PdaSeed = lez_authority::seed(b"owner-example/config/v1");

#[derive(Default, BorshSerialize, BorshDeserialize)]
struct Config {
    value: u64,
    owner: AuthoritySlot,
}

impl HasSlot<Owner> for Config {
    fn slot(&self) -> &AuthoritySlot {
        &self.owner
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.owner
    }
}

#[derive(BorshSerialize, BorshDeserialize)]
enum Message {
    SetValue(u64),
    Owner(Op),
    Forwarded(Forwarded),
}

impl Hosts for Message {
    fn forwarded(hop: Forwarded) -> Self {
        Message::Forwarded(hop)
    }
    fn into_forwarded(self) -> Result<Forwarded, Self> {
        match self {
            Message::Forwarded(hop) => Ok(hop),
            other => Err(other),
        }
    }
    fn into_op(self) -> Result<Op, Self> {
        match self {
            Message::Owner(op) => Ok(op),
            other => Err(other),
        }
    }
}

fn handle(ctx: &mut Ctx, config: &mut Config, message: Message) {
    match message {
        Message::SetValue(value) => {
            ctx.require(&config.owner);
            config.value = value;
        }
        Message::Owner(_) | Message::Forwarded(_) => unreachable!("the guard handles these"),
    }
}

fn entry(input: &ReceiveInput, message: Message) -> Response {
    lez_authority::guard::<Owner, Config, Message>(
        input,
        &CONFIG_SEED,
        InitGate::Deployer,
        message,
        handle,
    )
}

lee_core::actor_entrypoint!(entry);
```

The guard forwards at seats, unwraps the hop at the config, runs the hosted operations on the slot without the handler, calls the handler for everything else, and refuses a result whose slot changed by any means other than the context. A program whose state also lives on other accounts hosts `Hub` as well and uses `guard_with_targets`. See [lez-admin-authority](https://github.com/mmlado/lez-admin-authority) for a full program with targets and its client.

## The slot

```rust
pub enum AuthoritySlot {
    Unset,
    Held { holder: Holder, nominee: Option<AccountId> },
    Renounced,
}

pub struct Holder {
    pub account: AccountId,
    pub via: Option<AccountId>,
}
```

`Unset` is the default before the first install. `Held` has one holder and at most one standing offer of the role, the nominee. `Renounced` is for good. The four management operations, hosted as `Op` in the program's enum and run by the guard:

| Operation | Who | Effect |
| --- | --- | --- |
| `Init` | the deployer, or the genesis key, at top level | installs the sender as holder of an unset slot and creates the config |
| `Nominate { next }` | the holder | records the standing offer, the all-zero id cancels it |
| `Accept { pda_seed }` | the nominee, through its own seat | takes the role, a PDA proves itself with the seed that derives it under the program that reached its seat |
| `Renounce` | the holder | empties the slot for good, offer included |

There is no one-step transfer: a PDA cannot co-sign a transaction, and one shape serves keys and PDAs alike.

`InitGate::Deployer` lets only the program's own account install the first holder. On LEZ 0.4 the program id is the loader header account, and the deployer holds its key, so nobody can win the race after deployment. `InitGate::Genesis(key)` is the opt-in alternative for a program whose header account is not a key: the key is compiled in and is part of the program id.

## Refusals

A refusal is a panic, which is how a handler fails on LEZ. The text is `lez-authority/<role> E<code>: <sentence>`, and it reaches the sequencer log verbatim. The transaction is included, reverted, and charged. Clients see no error: they read the state back. Codes are append-only, a number is never reused or renumbered, and `codes_are_stable` pins the table.

| Code | Refusal | When |
| --- | --- | --- |
| 101 | `NotInitialized` | the config account holds no state and the message is not `Init` |
| 102 | `Renounced` | the slot was renounced |
| 103 | `NotHolder` | the caller is not the holder |
| 104 | `MissingSignature` | the sender did not authorize the transaction at its own actor |
| 107 | `SeedProofFailed` | a nominee reached through a program did not prove itself with the seed that derives it there |
| 108 | `MessageDoesNotDecode` | a message does not encode or decode |
| 109 | `ConfigDoesNotDecode` | the config account's bytes do not decode as the config |
| 113 | `NoNomination` | no nominee is pending |
| 114 | `NotNominee` | the caller is not the pending nominee |
| 115 | `NotGenesis` | `Init` from an account other than the genesis key, or not at top level |
| 117 | `NotDeployer` | `Init` from an account other than the program's own, or not at top level |
| 118 | `AlreadyInstalled` | `Init` on a held slot |
| 119 | `NotOwnProgram` | a forwarded message came from outside this program |
| 120 | `ViaNotAllowed` | the holder's account, reached through the wrong program |
| 121 | `ForwardedNotAtConfig` | a forwarded message arrived somewhere other than the config account |
| 122 | `NotThroughSeat` | a message that needs the holder was sent straight to the config |
| 123 | `NotFromConfig` | a hub message came from somewhere other than the config account, or reached the config |
| 126 | `SlotChanged` | the handler changed the slot by other means than the context |
| 131 | `NotInstalled` | the slot holds nobody |
| 132 | `SendNeedsHolder` | the handler sent to a target before the holder was verified |
| 133 | `TargetDoesNotDecode` | the target's state does not decode |

Numbers up to 116 follow the v0.2 `AdminError` table where a refusal has the same meaning there. The gaps are v0.2 codes with no counterpart here.

`guard` panics. `try_guard` returns the guard's own refusals as a value, for host tests. The context always panics.

## Security properties

| Property | What it stops |
| --- | --- |
| Identity only from the engine's `from`, at the seat and at the config | a message naming a forged holder or program |
| The seat needs `is_authorized`, the config needs `via == holder.via` exactly | the holder signs for program Q, and Q reaches the holder's seat (E120) |
| A forward is accepted only from an actor of the same program, and only at the config | a root or foreign forward (E119), a forward aimed at a seat (E121) |
| The empty config account accepts only `Init`, under the gate | squatting the config before the deployer installs (E101, E117, E115) |
| `Init` refuses a held or renounced slot | re-initialization (E118, E102) |
| A target accepts a hub message from the config account only, by exact actor | another actor of the program posing as the config (E123) |
| `send` needs `require` first | a handler reaching targets on an open message (E132) |
| The slot is compared around the handler | a handler replacing the holder (E126) |
| The seed proof on a PDA accept, and `via` on the holder | a program that inherited a grant accepting on a PDA's behalf (E107), or acting for it later (E120) |
| The context sends to actors of its own program only | a PDA holder's inherited grant reaching a foreign program through the handler |
| Whole-transaction revert | a program whose seeded call fails at the config also loses its own state change |

Holes the library cannot close: a program deployed with a mutable header has a super-admin in the header key, so deploy immutable or keep that key under the holder's governance. A private actor may present a blinded alias as `from`, so the check fails closed and only public holders are supported.

## What is not here

- Local messages, a message that runs at the actor that received it without a hop: [issue #1](https://github.com/mmlado/lez-authority/issues/1).
- Private holders and private config accounts.
- Any client code. The library is guest code only. The admin repo's runner is the reference client.

## Versioning and stability

Semantic versioning covers the public Rust API, the Borsh encodings of `AuthoritySlot`, `Holder`, `Op`, `Forwarded` and `Hub`, which are on-chain wire formats inside the program's own config and message enum, and the refusal codes with their meaning. While the version is 0.x, a minor bump may change any of them, and each such change is called out in the [changelog](CHANGELOG.md).

The `lee_core` dependency pins logos-execution-zone `dev` at commit `7e3f7f765371c4110a1f7f37baf520cd253fb339`. A consumer must pin the same commit, spelt the same way: cargo treats a tag and a rev as different sources even when they point at the same commit, and two copies of `lee_core` do not compile. Every re-pin is a minor release.

## Development

```bash
cargo fmt --check
cargo clippy --workspace --all-targets --locked
cargo test --workspace --locked
```

Host tests only: the engine runs the same code the guest does, and the admin repo builds the guest and runs it on a local dev sequencer.

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache 2.0](LICENSE-APACHE2) at the consumer's option.
