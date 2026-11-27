# lez-authority

[![CI](https://github.com/mmlado/lez-authority/actions/workflows/ci.yml/badge.svg)](https://github.com/mmlado/lez-authority/actions/workflows/ci.yml)
[![Consumer build](https://github.com/mmlado/lez-authority/actions/workflows/consumer-build.yml/badge.svg)](https://github.com/mmlado/lez-authority/actions/workflows/consumer-build.yml)
[![Audit](https://github.com/mmlado/lez-authority/actions/workflows/audit.yml/badge.svg)](https://github.com/mmlado/lez-authority/actions/workflows/audit.yml)
[![Version](https://img.shields.io/github/v/tag/mmlado/lez-authority?include_prereleases&sort=semver&label=version)](https://github.com/mmlado/lez-authority/releases)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue)](LICENSE-MIT)
[![Rust](https://img.shields.io/badge/rust-1.98.1-93450a?logo=rust)](rust-toolchain.toml)

This repository holds three crates that programs depend on, released together at one version:

| Crate | What a program depends on it for |
| --- | --- |
| [`lez-authority`](#lez-authority) | Roles of its own, on the engine below: the slot, the hop to the config and the guard |
| [`lez-admin-authority`](lez-admin-authority/README.md) | An admin, installed by the deployer, handed over by nominate and accept, renounceable |
| [`lez-freeze-authority`](lez-freeze-authority/README.md) | A freeze authority beside the admin, which freezes the whole program or single accounts and releases them |

The engine, lez-authority on top of lez-authority-core, holds the authority primitives for LEZ actor programs (LEZ 0.4): a slot per role inside a program's config, the hop that carries a sender's authority from their own actor to the config, and the guard that runs in front of the program's own handler. No notion of admin or freeze lives here. No SPEL dependency.

Status: `v0.2.0`. See [Versioning and stability](#versioning-and-stability) for what a release promises. Pinned to logos-execution-zone `v0.4.0-rc3` (2026-11-11), the actor model of PR #935. The v0.2.4 line lives in [spel-authority](https://github.com/mmlado/spel-authority) and stays as it is.

See [CONTEXT.md](CONTEXT.md) for the glossary, [CONTEXT-MAP.md](CONTEXT-MAP.md) for the admin's and the freeze's beside it, [docs/routing.md](docs/routing.md) for every hop with its checks, and [docs/adr](docs/adr) for the design decisions.

## The problem

On LEZ 0.4 a program is one handler that runs at an actor, the pair of an account and the program. The runtime hands the handler the message, the actor that sent it (`from`), and one bit about the receiving account: whether it authorized the transaction (`is_authorized`). It says nothing about any other signer. So a program that keeps its state on one well-known account cannot ask, at that account, whether the admin signed. The admin's signature is visible only at the admin's own actor.

## The model

A message that needs the holder of a role goes to the sender's own actor under the program first, the seat. The seat checks `is_authorized` and forwards the message to the config account, naming the program that reached the seat. The config trusts the forward because only an actor of the same program can produce one, and every actor of the program runs the same code: the guard in front of the program's handler.

```text
               seat (sender, P)              config (C, P)                      target (T, P)
client ──────► is_authorized?     hop       from an actor of P?       send      from == (C, P)?
               via = the program ─────────► caller = from.account ──────────► handler(input, message)
               that reached me              holder checks
                                            the refusing role's may_run
                                            handler(input, message)
                                            slots unchanged?
```

Two hops for a message that needs a holder. One for any other message, which runs where it is sent, and two when a role refuses program messages, since it then asks the config for clearance. Three when the handler at the config sends on to another actor of the program, and four with that request. Identity comes from the engine's `from` at every hop, never from a field in the message.

A holder is an account together with the program it acts through, its `via`: none for a key that signs at top level, the owning program for a PDA. The guard accepts a caller only when both match ([lez-authority ADR-0001](docs/adr/0001-per-holder-via.md)).

## Adding as a dependency

```toml
[dependencies]
borsh = { version = "1", features = ["derive"] }
lee_core = { git = "https://github.com/logos-blockchain/logos-execution-zone", tag = "v0.4.0-rc3" }
lez-authority = { git = "https://github.com/mmlado/lez-authority", tag = "v0.2.0" }
```

A program that needs an admin depends on [lez-admin-authority](lez-admin-authority/README.md) instead, and one that also needs a freeze authority on [lez-freeze-authority](lez-freeze-authority/README.md). Both bring this crate in. The `lee_core` source must match the one this crate pins, see [Versioning and stability](#versioning-and-stability).

## What a program writes

The program stays a plain LEZ program: its own message enum, its own state, and a handler `fn(&ReceiveInput, Message) -> Response` under `lee_core::actor_entrypoint!`. It adds a slot to its config, marks the messages that need a holder, and puts the `guard` attribute on the handler ([lez-authority ADR-0004](docs/adr/0004-the-guard-in-front-of-a-plain-handler.md)). A downstream crate names the role and its rules, as lez-admin-authority does for `Admin`. The whole of [`examples/owner.rs`](examples/owner.rs):

```rust
use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::program::{PdaSeed, ReceiveInput, Response};
use lez_authority::{AuthoritySlot, Gated, HasSlot, Hooks, Policy, Role};

/// The role this program has one holder of.
pub struct Owner;

impl Role for Owner {
    const NAME: &'static str = "owner";
    type Slot = AuthoritySlot;
}

/// The owner answers to nobody, so the default rules hold.
impl<C: HasSlot<Owner>> Policy<C> for Owner {}
impl<C: HasSlot<Owner>> Hooks<C> for Owner {}

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
| A role's operation: `Init`, `Nominate`, `Accept`, `Renounce`, or one the role defines for itself | sent to the sender's seat and forwarded. The guard runs it at the config: `Accept` for the nominee alone, the other three under the role's policy, and one of the role's own through its hooks. The handler never sees it |
| One marked `#[only(R)]` | sent to the sender's seat and forwarded. At the config the guard checks `R`'s holder and the consent of the role that refuses program messages, unless the message is exempt from it, then the handler runs |
| One marked `#[from_config]` | at another actor of the program, only when the config account sent it |
| A role's own message, which only the role's consent or operation sends, built with `Ctx::own_message` | at the role's own actor for its key, only when the config account sent it. The guard hands it to the role's `Hooks::receive`, and the handler never sees it |
| Anything else | where it is sent, as in a program without the guard, and under a role that refuses program messages it asks the config for clearance |

At the config every program message needs an initialized config and the consent of the role that refuses program messages, unless it is exempt from it, and the guard refuses a handler whose write changed a slot. The handler of a message that came through a seat sees `input.from` as the sender's own actor and `input.message` as the hop. Wherever the handler runs, the guard refuses a response that sends the config a library message, which the config would take for a hop from that actor, and one that sends a role's own message to an actor of the program (E140). Each role that watches state also looks at the actor's state before the handler and at its write after it. A client builds a role's operation with `Wire::op::<R>(op)`.

Away from the config, a program message handled there asks the config for clearance when a role refuses program messages and the message is not exempt from it, whoever sent it, the config included. The guard adds the request to the handler's response, and the config answers it under that role's rules in the same transaction, so a refusal reverts the whole transaction, the handler's effects with it. A client lists the config actor in such a transaction ([lez-authority ADR-0005](docs/adr/0005-a-message-handled-elsewhere-asks-the-config-for-clearance.md)). A request from a private actor runs at the config as a public segment and adds its crossing to the transaction.

## Roles

A role is a type that implements three traits. `Role` names it in refusals and in a client's operation message, and gives the type of its slot. `Policy<C>` says who may install, nominate and renounce, and what renounce leaves. `Hooks<C>` says what the role adds to the guard: what its own operations and messages do, and which program messages may run, over which state and with which writes. `Role::PROSE_NAME` names the role in a refusal's sentence where its name alone does not read well, as "freeze authority" does for `freeze`. `Policy::BY_NOMINATION_ONLY` marks a role whose holder is only ever appointed by nomination: its `Init` is refused with E137, and the guard attribute refuses a first role with it where `cargo check` sees it, at its `roles(..)`. A guard called by hand is refused at `cargo build`. The defaults are the rules of a role that answers to nobody, defines no operations or messages of its own and lets every program message run and every write stand, so such a role implements `Policy` and `Hooks` empty, as `Owner` does above. `HasSlot<R>` puts the role's slot in the config. A role that keeps only its holder takes `AuthoritySlot` as its slot. One that keeps more, as a freeze authority keeps what it froze, takes a type of its own that implements `Slot`, which hands the guard the `AuthoritySlot` inside it.

A role may define operations of its own, as a freeze authority defines a freeze. A client sends one as `Op::Own`, in the role's own encoding, and the guard runs it at the config as it runs `Nominate`: through the sender's seat, and never before the root role's `Init`. Its bytes run to the end of the message with no length in front, so a client's IDL can describe them as the role's own type. `Hooks::operate` decodes them, applies the role's rules and returns the role's slot as the operation leaves it, with the sync messages the config sends along with its new state. The guard checks no holder first, so those rules say who may send each operation. The guard writes back that slot alone and keeps its holder part as it was: the holder moves only through `Init`, `Nominate`, `Accept` and `Renounce`. A role that defines none refuses one with E108, a message that does not decode.

A role may also refuse program messages through its consent, as a freeze authority refuses every one while the program is frozen. It says so with `Hooks::REFUSES_MESSAGES`, and at the config the guard then asks its `Hooks::may_run` after the holders a message needs, and before the handler runs. A guard takes one such role at most, and the attribute refuses a second where `cargo check` sees it. Away from the config the guard asks the same rules through a request for clearance, so the role is enforced wherever a program message runs. A message marked `#[exempt(R)]` skips role `R`'s answer, and still needs its holders and leaves the slots alone. A role that watches state still looks at the state around its handler. A management operation never reaches `may_run`, so a role's operations always run.

The consent of the role that refuses program messages may come with sync messages, which `may_run` returns. The config sends them ahead of the handler's own, or alone at a request for clearance, in the same transaction, so a refusal there refuses the message too. Like the handler's, they carry the grants the config's delivery came with. `Ctx::config` gives the config's actor, which also names the program. The guard refuses a role whose consent or operation sends any actor of the program a message (E139), but for the role's own message to an actor other than the config. Every actor of the program takes any other message from the config as already gated: a `#[from_config]` message runs, and so does one that needs a holder. A role's rules pass no other role's gate, so they reach the program only through the role's own messages, at the role's own actors.

A role's own message, built with `Ctx::own_message::<R>(key, bytes)`, carries bytes in the role's own encoding from the config to the role's own actor for a 32-byte key, as a freeze authority's check goes to the actor that marks a frozen account. That actor is a PDA of the program whose seed hashes a fixed tag, the config's account, the key and the role's name, and `own_actor::<R>(config, key)` gives it to a client. The message names the seed, so it reaches the actor before the actor holds any state. The guard there takes it only from the config, by exact actor, and never at the config (E123), and only at that actor (E141). It hands the bytes to the role's `Hooks::receive` with that actor's state, and writes back the state that returns, or keeps the state for `None`. So a role writes its own actors alone, never a seat, a balance or another role's actor. The handler never sees the message. A handler that sends one to an actor of the program is refused (E140), since one sent from the config would pose as the role. A role that defines none refuses one with E108.

A role may also watch the state of every actor the handler runs at, as a freeze authority keeps the program off the actor that marks a frozen account. It says so with `Hooks::WATCHES_STATE`. The guard then asks its `Hooks::may_handle` with the actor's state just before the handler, at every actor of the program, the config included. After the handler it asks its `Hooks::may_write` with the state before and the state written, or `None` when the handler kept the state. A refusal in either refuses the message. So the role refuses before a handler that cannot read its state fails on it. Neither look is told which actor it runs at, so a role refuses by the state it recognizes, and an actor that holds none of the role's state runs any handler. Both looks cover exempt messages too. A message that runs no handler, such as a management operation, is never looked at. A program whose roles leave the switch unset pays nothing for the looks.

A program names its roles in the attribute: `roles(A)` for one, `roles(A, B)` for two. The first is the root. Its `Init` creates the config, and the guard's own refusals name it. A second role that answers to the first overrides the rules that differ and reads the first role's slot through the config, the way a freeze authority answers to the admin. A message may need either role's holder, or both ([lez-authority ADR-0003](docs/adr/0003-roles-compose-through-traits.md)).

A role crate, one that defines roles for programs to use, is named `lez-<role>-authority` and re-exports this crate at its root as `lez_authority`: with `pub use lez_authority;`, or through the role crate it builds on, as lez-freeze-authority does with `pub use lez_admin_authority::lez_authority;`. The macros find this crate through it, so a program may depend on the role crate alone.

## Layers

The engine stands in five layers, each naming only the layers below it ([lez-authority ADR-0008](docs/adr/0008-the-engine-stands-in-layers-that-import-downward.md)). From the bottom:

| Layer | What it holds | Its counterpart on the SPEL line |
| --- | --- | --- |
| lez-authority-core, in [`core/`](core/) | a role, its slot and holder, the holder's four transitions, the init gate and the refusal codes | spel-authority, policy-free as this layer is |
| [`wire`](src/wire.rs) | the library's messages and their encoding behind the byte 255 | the management instructions each role crate shipped, which SPEL merged into the program's dispatcher and IDL |
| [`policy`](src/policy.rs) | `Policy`, `Ctx` and `Caller`, and the four holder operations | each role crate's own rules on top of spel-authority, such as who may transfer the admin |
| [`hooks`](src/hooks.rs) | `Hooks`: a role's own operations and messages, its consent and its looks at state, with the rules the guard keeps around each | SPEL's extension mechanism ([logos-co/spel#257](https://github.com/logos-co/spel/pull/257)), through which spel-freeze-authority put its freeze check in front of the program's instructions |
| [`guard`](src/guard.rs) | where each message runs, the hop, the dispatch of each operation and the checks on the way | SPEL's dispatch, with each role crate's gate in front of the handler body, such as `#[require_admin]`. The hop is new on LEZ 0.4 |

The SPEL line is spel-authority, spel-admin-authority and spel-freeze-authority at v0.2.0, on SPEL v0.7.0.

The core is a crate of its own, so the compiler keeps it from naming anything above it. The other four are modules of lez-authority, and [`src/tests/layers.rs`](src/tests/layers.rs) fails on a module that names one above it, by a path, an alias or a glob of the crate root, a method or const a module above adds to one of its types, a trait a module above implements for one of them, a path in an attribute, or a doc link. It also fails on code it cannot read, such as an included file, a doc comment in a block, or an attribute on a module's declaration. A program sees one flat API, since lez-authority re-exports every item at its root.

## The macros

| Macro | On | Writes |
| --- | --- | --- |
| `#[guard(roles(..), config = C, seed = S)]` | the handler | the outer function and the call to `guard`. `genesis = KEY` replaces the deployer gate |
| `#[derive(HasSlot)]`, `#[slot(R)]` | the config, its slot field | `HasSlot<R>` |
| `#[derive(Gated)]`, `#[only(R)]`, `#[from_config]`, `#[exempt(R)]` | the message enum, its variants | `Gated` |

`#[only]` and `#[exempt]` take one role each, and it must be one of the guard's roles. The guard knows a role by its name, as a client's operation does, so a program's roles need a name each, and a role crate linked twice is still one role. A misplaced marker is a compile error with the reason, such as `a message is either #[only(..)], sent through the sender's seat, or #[from_config], sent by the config`, and the attribute refuses a marker naming a role it does not list where `cargo check` sees it, and an `#[exempt(R)]` naming a role that refuses no program messages. [`tests/macros.rs`](tests/macros.rs) holds the derives against hand-written impls, and a handler under the attribute against its hand expansion, input by input, refusals included. The macros find this crate through a direct dependency on it, or on a role crate (see [Roles](#roles)). They take one that every build links: required dependencies first, then those for a target, then optional ones, then dev-dependencies, and within each of these, this crate ahead of a role crate. A program whose dependencies do not fit the rule depends on this crate directly, at the same tag as its role crate, and the macros take it first. A build script's macros look in its build-dependencies instead. A new role crate needs no change here.

## The slot

```rust
pub enum AuthoritySlot {
    Unset,
    Held { holder: Holder, nominee: Option<AccountId> },
    Renounced,
    Offered { nominee: AccountId },
}

pub struct Holder {
    pub account: AccountId,
    pub via: Option<AccountId>,
}
```

`Unset` is the default before the first install. `Held` has one holder and at most one standing offer of the role, the nominee. `Renounced` is for good. `Offered` has no holder yet and a standing offer, which a slot gets where the role's policy lets someone who is not the holder nominate. The four management operations, sent as library messages and run by the guard. The "who" is the default `Policy`:

| Operation | Who | Effect |
| --- | --- | --- |
| `Init` | the deployer, or the genesis key, at top level | installs the sender as holder of a slot that has none, keeping a standing offer. The root role's `Init` creates the config |
| `Nominate { next }` | the holder | records the standing offer, the all-zero id cancels it. On a slot with no holder, where the policy allows it, the offer makes the slot `Offered` |
| `Accept { pda_seed }` | the nominee, through its own seat | takes the role, a PDA proves itself with the seed that derives it under the program that reached its seat |
| `Renounce` | the holder | resets the holder part, offer included, by default for good. Whatever else the role keeps in its slot stays |

A role's own operations, sent as `Own`, change the rest of its slot under the rules its `Hooks::operate` sets, never the holder part.

There is no one-step transfer: a PDA cannot co-sign a transaction, and one shape serves keys and PDAs alike.

`InitGate::Deployer` lets only the program's own account install the first holder. On LEZ 0.4 the program id is the loader header account, and the deployer holds its key, so nobody can win the race after deployment. `InitGate::Genesis(key)` is the opt-in alternative for a program whose header account is not a key: the key is compiled in and is part of the program id.

## Refusals

A refusal is a panic, which is how a handler fails on LEZ. The text is `lez-authority/<role> E<code>: <sentence>`, with the role's name in the prefix and its prose name in the sentence, as in `lez-authority/freeze E103: the signing account is not the freeze authority`. A role crate's own refusals print with that crate's prefix, as in `lez-freeze-authority/freeze E217: the program is frozen`. The LEZ wallet runs a transaction before it sends it and, by default, does not send one that reverts: the text comes back in its `SimulatedRevert` error. A transaction sent anyway is included, reverted, and charged its whole gas limit, the text reaches the sequencer log verbatim, and the client reads the state back. Codes are append-only, a number is never reused or renumbered, and `codes_are_stable` pins the table.

| Code | Refusal | When |
| --- | --- | --- |
| 101 | `NotInitialized` | the config account holds no state and the message is not the root role's `Init` |
| 102 | `Renounced` | the slot was renounced |
| 103 | `NotHolder` | the caller is not the holder |
| 104 | `MissingSignature` | the sender did not authorize the transaction at its own actor |
| 107 | `SeedProofFailed` | a nominee reached through a program did not prove itself with the seed that derives it there |
| 108 | `MessageDoesNotDecode` | a message does not decode, names a role the program does not have, or is an operation or own message its role does not define |
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
| 123 | `NotFromConfig` | a `#[from_config]` message, or a role's own message, came from somewhere other than the config account, or reached the config |
| 126 | `SlotChanged` | the handler's write changed a slot |
| 131 | `NotInstalled` | the slot holds nobody |
| 135 | `LibrarySend` | the handler's response sent a library message to the config account |
| 136 | `ClearanceFromOutside` | a request for clearance from outside the program |
| 137 | `ByNominationOnly` | `Init` of a role whose holder is only ever appointed by nomination |
| 138 | `ClearanceNotAtConfig` | a request for clearance arrived somewhere other than the config account |
| 139 | `RoleSendsInProgram` | a role's consent or own operation sent an actor of the program a message other than the role's own, or sent its own to the config |
| 140 | `HandlerSendsOwnMessage` | the handler's response sent a role's own message to an actor of the program |
| 141 | `NotOwnActor` | a role's own message reached an actor other than the role's own actor for its key |

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
| Only a seat sends the config a hop, wherever the handler runs | an open message at a holder's actor, sent unsigned, whose handler passes a client's bytes on as a hop the config would take for the holder's (E135) |
| A program message handled away from the config asks the config for clearance when a role refuses program messages, the guard adding the request after the handler, and the config answers only the program's own actors (E136) | a freeze with a side door, a message at another actor running while the program is frozen, or a handler leaving out its own request |
| A role's sync messages go to an actor of the program only as the role's own message, and never to the config | a role's consent or operation posing as a hop, or sending a `#[from_config]` or gated message that every actor of the program takes as already gated, such as a credit minted with no admin, or posing as another role (E139) |
| A role's own message runs only at the role's own actor for its key, only when the config sent it, and no handler sends one in the program | a role writing a seat, a balance or another role's actor, or a client or a handler posing as the role, such as a handler at the config writing the role's state (E123, E140, E141) |
| A role that sets `WATCHES_STATE` looks at the actor's state before every handler and at the handler's write after it, at every actor, exempt messages included | the program running at an actor whose state a role recognizes, or writing such state, such as a credit that would erase a freeze's mark or forge one (the role's own code) |
| Every slot is compared around the handler | a handler replacing a holder, or changing what a role keeps in its slot (E126) |
| A role's own operation writes back that role's slot alone, with its holder part kept | an operation reviving a renounced holder, or moving the holder past the nomination, the accept and the seed proof |
| The `may_run` of the role that sets `REFUSES_MESSAGES` answers at the config for every program message not exempt from it, after the holders, and never for a management operation | a program message running through a freeze at the config, or a freeze stopping its own release |
| Every role a marker names is one of the guard's roles, checked at compile time, and markers and the guard compare roles by name | an `#[only]` message whose role the guard never asks, running for anyone, such as one marked for the freeze under a guard that lists only the admin, or one marked through a second copy of a role crate |
| A guard's roles have a name each, checked at compile time | an operation meant for one role running on another role's slot |
| The seed proof on a PDA accept, and `via` on the holder | a program that inherited a grant accepting on a PDA's behalf (E107), or acting for it later (E120) |
| Whole-transaction revert | a program whose seeded call fails at the config also loses its own state change |

Holes the library cannot close: a program deployed with a mutable header has a super-admin in the header key, so deploy immutable or keep that key under the holder's governance. A private actor may present a blinded alias as `from`, so the check fails closed and only public holders are supported.

A PDA holder's grant comes with the call, so the handler, and every program it calls, holds the PDA's authority for that call. Make a PDA holder an account that holds no assets and no other role. A program that received the grant cannot act as the holder here: it reaches the holder's seat with itself as via (E120).

## What is not here

- A check inside a handler arm, or one that depends on the message's data. The guard checks a message by its variant and by what the roles keep in their slots, so a conditional rule splits into an open variant and an `#[only(R)]` one. A role that watches state sees only the state of the actor the handler runs at, before and after it, never the message.
- The root role's `Init` with the program's own settings. It writes the config's `Default` with the holder installed, so make the default a safe state and set the rest with `#[only(R)]` messages.
- Private holders and private config accounts.
- Client code beyond `Wire::op` and `own_actor`. The admin's sample runner, [`lez-admin-authority/sample/runner`](lez-admin-authority/sample/runner/), is the reference client.

## Versioning and stability

The three crates share one version and one tag, so a release of one is a release of all three, and a program names the same tag for each of them it depends on ([lez-authority ADR-0007](docs/adr/0007-one-repository-at-one-version.md)).

Semantic versioning covers the public Rust API, the Borsh encodings of `AuthoritySlot`, `Holder`, `Op` (whose `Own` bytes carry no length and run to the end of the message), `Forwarded` and `LibraryMessage` (whose `Own` bytes also run to the end), the seed `own_actor` hashes, the byte 255 that marks a library message, the refusal codes with their meaning, and the naming rule by which the macros find this crate through a role crate. Items hidden from the docs are not covered, since they serve the guard and its macros and change with them. The slot lives inside the program's own config, and the library messages travel next to the program's. For lez-admin-authority it also covers the Borsh encoding of `AdminOp` behind the byte 255. A program's own enum's encoding is the program's. While the version is 0.x, a minor bump may change any of them, and each such change is called out in the [changelog](CHANGELOG.md).

The `lee_core` dependency pins logos-execution-zone at its `v0.4.0-rc3` tag. A consumer must pin the same tag, spelt the same way: cargo treats a tag and a rev as different sources even when they point at the same commit, and two copies of `lee_core` do not compile. Every re-pin is a minor release.

## Development

The workspace holds the three crates, the engine's test fixtures, and each sample's program, types and smallest program. Its tests run on the host, on the same code the guests run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --locked
cargo test --workspace --locked
cargo check --locked --manifest-path lez-admin-authority/ci/consumer-fixture/guest/Cargo.toml
```

The tests run programs in a world, lez-authority-world in [`world/`](world/), which hands each transaction to LEZ's own executor ([lez-authority ADR-0009](docs/adr/0009-tests-run-programs-through-lez-s-own-executor.md)). It is test support and outside the versioning promise.

Each sample's runner is a workspace of its own, since it links the LEZ wallet stack, which needs `libpcsclite-dev` and is slow. A runner needs `r0vm` on its `PATH` when it runs, because the wallet runs each call through it before sending:

```bash
cargo build --release --locked --manifest-path lez-admin-authority/sample/runner/Cargo.toml
cargo build --release --locked --manifest-path lez-freeze-authority/sample/runner/Cargo.toml
```

The guests and the overhead numbers need the risc0 toolchain, installed through `rzup`:

```bash
cargo build --release --locked --manifest-path lez-admin-authority/sample/methods/Cargo.toml
cargo build --release --locked --manifest-path lez-freeze-authority/sample/methods/Cargo.toml
RISC0_DEV_MODE=1 cargo test --locked --manifest-path lez-admin-authority/sample/methods/Cargo.toml --test overhead -- --nocapture
RISC0_DEV_MODE=1 cargo test --locked --manifest-path lez-freeze-authority/sample/methods/Cargo.toml --test overhead -- --nocapture
```

CI runs the first block on pull requests and on main, and builds the runners and the guests weekly. On pull requests it also checks that the runners' and the guests' own locks are current and hold the root lock's risc0, the version LEZ pins for the sequencer. Each sample's `sample/RUN.md` runs it on a local dev sequencer.

A change to a crate's version or dependencies changes those locks too. Refresh them with `cargo fetch --manifest-path <folder>/Cargo.toml`, which keeps the versions they lock and changes only what the change needs. Do not run `cargo update` on them. LEZ pins risc0-zkos-v1compat at 2.2.2. Of the risc0-zkvm versions LEZ allows, only 3.0.5 takes it, and 3.0.5 is yanked, so a new resolve fails. When a LEZ bump moves risc0 in those locks, the root lock follows with `cargo update -p risc0-zkvm --precise <version>`, and the same for risc0-zkos-v1compat.

## License

All three crates are dual-licensed under [MIT](LICENSE-MIT) and [Apache 2.0](LICENSE-APACHE2) at the consumer's option.
