# lez-authority

[![CI](https://github.com/mmlado/lez-authority/actions/workflows/ci.yml/badge.svg)](https://github.com/mmlado/lez-authority/actions/workflows/ci.yml)
[![Audit](https://github.com/mmlado/lez-authority/actions/workflows/audit.yml/badge.svg)](https://github.com/mmlado/lez-authority/actions/workflows/audit.yml)
[![Version](https://img.shields.io/github/v/tag/mmlado/lez-authority?include_prereleases&sort=semver&label=version)](https://github.com/mmlado/lez-authority/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.98.1-93450a?logo=rust)](rust-toolchain.toml)

Authority primitives for LEZ actor programs (LEZ 0.4): a slot per role inside a program's config, the hop that carries a sender's authority from their own actor to the config, and the guard that runs in front of the program's own handler. Policy-free: no notion of admin or freeze lives here. No SPEL dependency.

Status: `v0.1.0`. See [Versioning and stability](#versioning-and-stability) for what a release promises. Pinned to logos-execution-zone `dev` at 7e3f7f7 (2026-10-24), the actor model of PR #935. The v0.2.4 line lives in [spel-authority](https://github.com/mmlado/spel-authority) and stays as it is. First consumer: [lez-admin-authority](https://github.com/mmlado/lez-admin-authority), which names the admin role.

See [CONTEXT.md](CONTEXT.md) for the glossary, [docs/routing.md](docs/routing.md) for every hop with its checks, and [docs/adr](docs/adr) for the design decisions.

## The problem

On LEZ 0.4 a program is one handler that runs at an actor, the pair of an account and the program. The runtime hands the handler the message, the actor that sent it (`from`), and one bit about the receiving account: whether it authorized the transaction (`is_authorized`). It says nothing about any other signer. So a program that keeps its state on one well-known account cannot ask, at that account, whether the admin signed. The admin's signature is visible only at the admin's own actor.

## The model

A message that needs the holder of a role goes to the sender's own actor under the program first, the seat. The seat checks `is_authorized` and forwards the message to the config account, naming the program that reached the seat. The config trusts the forward because only an actor of the same program can produce one, and every actor of the program runs the same code: the guard in front of the program's handler.

```text
               seat (sender, P)              config (C, P)                      target (T, P)
client ──────► is_authorized?     hop       from an actor of P?       send      from == (C, P)?
               via = the program ─────────► caller = from.account ──────────► handler(input, message)
               that reached me              holder checks
                                            handler(input, message)
                                            slots unchanged?
```

Two hops for a message that needs a holder. One for any other message, which runs where it is sent. Three when the handler at the config sends on to another actor of the program. Identity comes from the engine's `from` at every hop, never from a field in the message.

A holder is an account together with the program it acts through, its `via`: none for a key that signs at top level, the owning program for a PDA. The guard accepts a caller only when both match ([ADR-0001](docs/adr/0001-per-holder-via.md)).

## Adding as a dependency

```toml
[dependencies]
borsh = { version = "1", features = ["derive"] }
lee_core = { git = "https://github.com/logos-blockchain/logos-execution-zone", rev = "7e3f7f765371c4110a1f7f37baf520cd253fb339" }
lez-authority = { git = "https://github.com/mmlado/lez-authority", tag = "v0.1.0" }
```

A program that only needs an admin depends on [lez-admin-authority](https://github.com/mmlado/lez-admin-authority) instead, which brings this crate in. The `lee_core` source must match the one this crate pins, see [Versioning and stability](#versioning-and-stability).

## What a program writes

The program stays a plain LEZ program: its own message enum, its own state, and a handler `fn(&ReceiveInput, Message) -> Response` under `lee_core::actor_entrypoint!`. It adds a slot to its config, marks the messages that need a holder, and puts the `guard` attribute on the handler ([ADR-0004](docs/adr/0004-the-guard-in-front-of-a-plain-handler.md)). A downstream crate names the role and its rules, as lez-admin-authority does for `Admin`. The whole of [`examples/owner.rs`](examples/owner.rs):

```rust
use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::program::{PdaSeed, ReceiveInput, Response};
use lez_authority::{AuthoritySlot, Gated, HasSlot, Policy, Role};

/// The role this program has one holder of.
pub struct Owner;

impl Role for Owner {
    const NAME: &'static str = "owner";
}

/// The owner answers to nobody, so the default rules hold.
impl<C: HasSlot<Owner>> Policy<C> for Owner {}

const CONFIG_SEED: PdaSeed = lez_authority::seed(b"owner-example/config/v1");

#[derive(Default, BorshSerialize, BorshDeserialize, HasSlot)]
struct Config {
    value: u64,
    #[slot(Owner)]
    owner: AuthoritySlot,
}

#[derive(BorshSerialize, BorshDeserialize, Gated)]
enum Message {
    #[only(Owner)]
    SetValue(u64),
}

#[lez_authority::guard(roles(Owner), config = Config, seed = CONFIG_SEED)]
fn handle_message(input: &ReceiveInput, message: Message) -> Response {
    let mut config: Config = borsh::from_slice(&input.pre_state).expect("the config decodes");
    match message {
        Message::SetValue(value) => config.value = value,
    }
    Response::set_state(borsh::to_vec(&config).expect("the config encodes"))
}

lee_core::actor_entrypoint!(handle_message);
```

[`examples/owner_by_hand.rs`](examples/owner_by_hand.rs) is the same program without the macros: the impls the derives write, and the call the attribute writes.

## What the guard does

The attribute turns the handler into an inner function and puts `lez_authority::guard` in front of it. The outer function takes `Wire<Message>`, so the library's own messages decode next to the program's. They start with the byte 255, which no message of an enum of fewer than 255 variants starts with, so the program's messages keep their encoding.

| Message | Where it runs |
| --- | --- |
| A role's operation: `Init`, `Nominate`, `Accept`, `Renounce` | sent to the sender's seat and forwarded. The guard runs it at the config under the role's policy, and the handler never sees it |
| One marked `#[only(R)]` | sent to the sender's seat and forwarded. At the config the guard checks `R`'s holder, then the handler runs |
| One marked `#[from_config]` | at another actor of the program, only when the config account sent it |
| Anything else | where it is sent, as in a program without the guard |

At the config every program message needs an initialized config, and the guard refuses a handler whose write changed a slot. The handler of a message that came through a seat sees `input.from` as the sender's own actor and `input.message` as the hop. Wherever the handler runs, the guard refuses a response that sends the config a library message, which the config would take for a hop from that actor. A client builds a role's operation with `Wire::op::<R>(op)`.

## Roles

A role is a type with `Role`, which names it in refusals and in a client's operation message, and `Policy<C>`, which says who may install, nominate and renounce, and what renounce leaves. The defaults are the rules of a role that answers to nobody, so such a role implements `Policy` empty, as `Owner` does above. `HasSlot<R>` puts the role's slot in the config.

A program names its roles in the attribute: `roles(A)` for one, `roles(A, B)` for two. The first is the root. Its `Init` creates the config, and the guard's own refusals name it. A second role that answers to the first overrides the rules that differ and reads the first role's slot through the config, the way a freeze authority answers to the admin. A message may need either role's holder, or both ([ADR-0003](docs/adr/0003-roles-compose-through-traits.md)).

## The macros

| Macro | On | Writes |
| --- | --- | --- |
| `#[guard(roles(..), config = C, seed = S)]` | the handler | the outer function and the call to `guard`. `genesis = KEY` replaces the deployer gate |
| `#[derive(HasSlot)]`, `#[slot(R)]` | the config, its slot field | `HasSlot<R>` |
| `#[derive(Gated)]`, `#[only(R)]`, `#[from_config]` | the message enum, its variants | `Gated` |

`#[only]` takes one role. A misplaced marker is a compile error with the reason, such as `a message is either #[only(..)], sent through the sender's seat, or #[from_config], sent by the config`. [`tests/macros.rs`](tests/macros.rs) holds the derives against hand-written impls, and a handler under the attribute against its hand expansion, input by input, refusals included. The macros find this crate through a role crate that re-exports it, so a program that depends on lez-admin-authority alone can use them.

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

`Unset` is the default before the first install. `Held` has one holder and at most one standing offer of the role, the nominee. `Renounced` is for good. The four management operations, sent as library messages and run by the guard. The "who" is the default `Policy`:

| Operation | Who | Effect |
| --- | --- | --- |
| `Init` | the deployer, or the genesis key, at top level | installs the sender as holder of an unset slot. The root role's `Init` creates the config |
| `Nominate { next }` | the holder | records the standing offer, the all-zero id cancels it |
| `Accept { pda_seed }` | the nominee, through its own seat | takes the role, a PDA proves itself with the seed that derives it under the program that reached its seat |
| `Renounce` | the holder | empties the slot, offer included, by default for good |

There is no one-step transfer: a PDA cannot co-sign a transaction, and one shape serves keys and PDAs alike.

`InitGate::Deployer` lets only the program's own account install the first holder. On LEZ 0.4 the program id is the loader header account, and the deployer holds its key, so nobody can win the race after deployment. `InitGate::Genesis(key)` is the opt-in alternative for a program whose header account is not a key: the key is compiled in and is part of the program id.

## Refusals

A refusal is a panic, which is how a handler fails on LEZ. The text is `lez-authority/<role> E<code>: <sentence>`, and it reaches the sequencer log verbatim. The transaction is included, reverted, and charged. Clients see no error: they read the state back. Codes are append-only, a number is never reused or renumbered, and `codes_are_stable` pins the table.

| Code | Refusal | When |
| --- | --- | --- |
| 101 | `NotInitialized` | the config account holds no state and the message is not the root role's `Init` |
| 102 | `Renounced` | the slot was renounced |
| 103 | `NotHolder` | the caller is not the holder |
| 104 | `MissingSignature` | the sender did not authorize the transaction at its own actor |
| 107 | `SeedProofFailed` | a nominee reached through a program did not prove itself with the seed that derives it there |
| 108 | `MessageDoesNotDecode` | a message does not decode, or names a role the program does not have |
| 109 | `ConfigDoesNotDecode` | the config account's bytes, or the handler's write, do not decode as the config |
| 113 | `NoNomination` | no nominee is pending |
| 114 | `NotNominee` | the caller is not the pending nominee |
| 115 | `NotGenesis` | `Init` from an account other than the genesis key, or not at top level |
| 117 | `NotDeployer` | `Init` from an account other than the program's own, or not at top level |
| 118 | `AlreadyInstalled` | `Init` on a held slot |
| 119 | `NotOwnProgram` | a forwarded message came from outside this program |
| 120 | `ViaNotAllowed` | the holder's account, reached through the wrong program |
| 121 | `ForwardedNotAtConfig` | a forwarded message arrived somewhere other than the config account |
| 122 | `NotThroughSeat` | an operation, or a message that needs a holder, was sent straight to the config |
| 123 | `NotFromConfig` | a `#[from_config]` message came from somewhere other than the config account, or reached the config |
| 126 | `SlotChanged` | the handler's write changed a slot |
| 131 | `NotInstalled` | the slot holds nobody |
| 135 | `LibrarySend` | the handler's response sent a library message to the config account |

Numbers up to 116 follow the v0.2 `AdminError` table where a refusal has the same meaning there. The gaps are v0.2 codes with no counterpart here, 132 and 133, retired with the handler context of `v0.1.0-alpha.1`, and 134, dropped before it was released.

`guard` panics. `try_guard` returns the guard's refusals as a `Refusal`, the error with the role it names, for host tests. The handler's own panics stay panics.

## Security properties

| Property | What it stops |
| --- | --- |
| Identity only from the engine's `from`, at the seat and at the config | a message naming a forged holder or program |
| The seat needs `is_authorized`, the config needs `via == holder.via` exactly | the holder signs for program Q, and Q reaches the holder's seat (E120) |
| A forward is accepted only from an actor of the same program, and only at the config | a root or foreign forward (E119), a forward aimed at a seat (E121) |
| An operation or a message that needs a holder is refused at the config unless it came through a seat | a holder check without the seat's signature check (E122) |
| The empty config account accepts only the root role's `Init`, under the gate | squatting the config before the deployer installs (E101, E117, E115) |
| `Init` refuses a held or renounced slot | re-initialization (E118, E102) |
| A `#[from_config]` message runs only when the config account itself sent it, by exact actor | another actor of the program, or a client, posing as the config (E123) |
| Only a seat sends the config a library message, wherever the handler runs | an open message at a holder's actor, sent unsigned, whose handler passes a client's bytes on as a hop the config would take for the holder's (E135) |
| Every slot is compared around the handler | a handler replacing a holder (E126) |
| The seed proof on a PDA accept, and `via` on the holder | a program that inherited a grant accepting on a PDA's behalf (E107), or acting for it later (E120) |
| Whole-transaction revert | a program whose seeded call fails at the config also loses its own state change |

Holes the library cannot close: a program deployed with a mutable header has a super-admin in the header key, so deploy immutable or keep that key under the holder's governance. A private actor may present a blinded alias as `from`, so the check fails closed and only public holders are supported.

A PDA holder's grant comes with the call, so the handler, and every program it calls, holds the PDA's authority for that call. Make a PDA holder an account that holds no assets and no other role. A program that received the grant cannot act as the holder here: it reaches the holder's seat with itself as via (E120).

## What is not here

- A check inside a handler arm, or one that depends on the message's data or the program's state. The guard checks a message by its variant, before the handler, so a conditional rule splits into an open variant and an `#[only(R)]` one.
- The root role's `Init` with the program's own settings. It writes the config's `Default` with the holder installed, so make the default a safe state and set the rest with `#[only(R)]` messages.
- Private holders and private config accounts.
- Client code beyond `Wire::op`. The admin repo's runner is the reference client.

## Versioning and stability

Semantic versioning covers the public Rust API, the Borsh encodings of `AuthoritySlot`, `Holder`, `Op`, `Forwarded` and `LibraryMessage`, the byte 255 that marks a library message, and the refusal codes with their meaning. The slot lives inside the program's own config, and the library messages travel next to the program's. While the version is 0.x, a minor bump may change any of them, and each such change is called out in the [changelog](CHANGELOG.md).

The `lee_core` dependency pins logos-execution-zone `dev` at commit `7e3f7f765371c4110a1f7f37baf520cd253fb339`. A consumer must pin the same commit, spelt the same way: cargo treats a tag and a rev as different sources even when they point at the same commit, and two copies of `lee_core` do not compile. Every re-pin is a minor release.

## Development

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked
cargo test --workspace --locked
```

Host tests only: the engine runs the same code the guest does, and the admin repo builds the guest and runs it on a local dev sequencer.

## License

Dual-licensed under [MIT](LICENSE-MIT) and [Apache 2.0](LICENSE-APACHE2) at the consumer's option.
