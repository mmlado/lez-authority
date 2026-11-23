# Authority lifecycle

How the admin slot moves between states over a program's lifetime, what each transition checks, and which guarantees the library gives. The hop-by-hop routing, with every code, is in [lez-authority's routing document](https://github.com/mmlado/lez-authority/blob/main/docs/routing.md).

## State machine

```text
        ┌──────────────────┐
        │  Uninitialized   │   the config account holds no bytes
        └────────┬─────────┘
                 │ Init, from the deployer's seat (or the genesis key's)
                 ▼
        ┌──────────────────────────────┐
        │  Held                        │   holder = { account, via }
        │  holder = A, nominee = None  │◄──────────────┐
        └────────┬─────────────────────┘               │
                 │ Nominate(N), signed by A            │ Nominate(all-zero id), signed by A
                 ▼                                     │
        ┌──────────────────────────────┐               │
        │  Held                        │───────────────┘
        │  holder = A, nominee = N     │
        └────────┬─────────────────────┘
                 │ Accept, signed by N through its own seat
                 ▼
        ┌──────────────────────────────┐
        │  Held                        │
        │  holder = N, nominee = None  │
        └────────┬─────────────────────┘
                 │ Renounce, signed by the holder, from any Held state
                 ▼
        ┌──────────────────┐
        │    Renounced     │   terminal, no further transitions
        └──────────────────┘
```

## States

**Uninitialized.** The config account, the PDA of the program id and the program's seed, holds no bytes. Only `Init` is accepted there, and only from the init gate. Every other message at the config, open ones included, is refused with E101 `NotInitialized`.

**Held.** The config exists and the slot is `AuthoritySlot::Held { holder, nominee }`. The holder is an account with its `via`: `None` for a key that signs at top level, the owning program for a PDA. Every admin check compares the caller with both. The nominee, when set, is the holder's standing offer and has no powers until it accepts.

**Renounced.** The config exists and the slot is `AuthoritySlot::Renounced`. Terminal. Every `#[only(Admin)]` message is E102, and so are `Accept`, `Nominate`, `Renounce` and `Init`. The rest of the config is untouched: open messages keep working.

`AuthoritySlot::Unset` is the slot's default value. It never reaches the chain through the guard: an empty config accepts only `Init`, which creates the config with the slot already held, and a handler cannot set the slot. It is what `Config::default()` holds in memory in the moment before `Init` installs the deployer.

## Transitions

### `Init`

```text
Uninitialized → Held { holder: { deployer, via: None }, nominee: None }
```

**Sent by:** the deployer, the program's own account, through its seat at top level. With `genesis = KEY` on the attribute, that key instead.

**Checks:** the seat is authorized (E104), the forward comes from an actor of the program (E119), the config account is empty, the caller's account is the gated one (E117, or E115 with a genesis key), and the caller's `via` is `None` (same codes). On a non-empty config the slot decodes and `Init` is E118 on a held slot, E102 on a renounced one.

**Effect:** the config is written as `Config::default()` with the slot held by the caller. There is no candidate argument: the deployer is always the first admin, and a program that wants another admin from the start hands over right after.

### `Nominate { next }`

```text
Held { holder: A, nominee: _ } → Held { holder: A, nominee: Some(next) }    or None for the all-zero id
```

**Sent by:** the admin, through its seat.

**Checks:** the admin check: not uninitialized (E101), not renounced (E102), the caller's account is the holder's (E103), the caller's `via` is the holder's (E120).

**Effect:** the nominee is recorded, replacing any earlier one. The admin does not change. Nothing is checked about `next`: it is a bare id, it need not sign or be present, and a mistyped one can never accept.

### `Accept { pda_seed }`

```text
Held { holder: A, nominee: Some(N) } → Held { holder: N, nominee: None }
```

**Sent by:** the nominee, through its own seat. A key signs at top level with `pda_seed: None`. A PDA is reached through the program that derives it, which sends the accept to the nominee's seat by a sync call carrying the seed as a grant, and `pda_seed` is that seed.

**Checks:** not uninitialized (E101), not renounced (E102), a nominee is pending (E113), the caller's account is the nominee (E114), and the seed proof (E107): a caller with `via: None` must carry no seed, a caller with `via: Some(program)` must carry the seed that derives its account under that program. Anything else, such as a key nominee reached through a program, fails the proof.

**Effect:** the nominee becomes the holder, with the `via` it accepted through, and the nominee field is cleared. The old admin is out at once.

### `Renounce`

```text
Held { .. } → Renounced
```

**Sent by:** the admin, through its seat.

**Checks:** the admin check, as for `Nominate`.

**Effect:** the slot is `Renounced`, nominee included. Terminal.

### An admin-only message

```text
Held → Held, the rest of the config changed by the handler
```

**Sent by:** the admin, through its seat.

**Checks:** the guard's, before the handler, for a message marked `#[only(Admin)]`: the message came through a seat (E122), then as for `Nominate`. After the handler returns, the guard refuses a write whose slot changed (E126), a library message to the config (E135), and a role's own message to an actor of the program (E140).

## State detection

| State | Config account | Slot |
| --- | --- | --- |
| Uninitialized | empty | none, nothing decodes |
| Held | the Borsh-encoded config | `Held { holder, nominee }` |
| Renounced | the Borsh-encoded config | `Renounced` |

A client reads the config actor's state with `get_account_view` and decodes the program's own `Config`. `AuthoritySlot` is a Borsh enum: one byte for `Unset` or `Renounced`, and for `Held` the tag, the holder's 32-byte account, its optional `via`, and the optional nominee.

The two empty-looking states are refused differently on purpose: E101 means "not yet", E102 means "never again".

## Re-initialization

`Init` on a non-empty config decodes the slot and refuses it: E118 when held, E102 when renounced. The config account's address is fixed by the program id and the seed, so there is no second address to initialize, and `Config::default()` is written only when the account is empty.

## How the admin's identity is established

The runtime sets `is_authorized` at an actor when the actor's account authorized the transaction: a key by signing it, a PDA by a grant from the program that derives it, carried on a sync message. That bit is visible only at the account's own actor, so the admin's message goes to the admin's seat first. The seat forwards to the config naming the program that reached it, or none at top level. The config takes the caller's account from the engine's `from`, never from the message, and accepts the forward only from an actor of its own program, which runs the same guard.

The admin is matched on both the account and the `via`. A key admin acts at top level only: a program that carries the admin's signature and reaches the admin's seat is refused (E120). A PDA admin acts through the program it accepted through, and only through it. See [ADR-0001](https://github.com/mmlado/lez-authority/blob/main/docs/adr/0001-per-holder-via.md).

## Program-as-admin

A PDA holds the admin role after accepting a nomination through its owning program. From then on, that program sends admin-only messages to the PDA's seat by a sync call with the PDA's seed, the seat sees the grant as authorization, and the config checks the holder's `via` against the program. A multisig vault is the expected case: on threshold, the vault program sends the admin message with the vault's seed, and the whole transaction reverts if the admin hop refuses.

Grants travel on sync messages only, so a vault whose execute step is an async message cannot hold the admin. The handler acting for a PDA admin, and every program it calls, holds the admin's authority for that call, so make the admin a PDA that holds no assets and no other role.

Both PDA paths, accept and act, are covered by host tests in both repos. They have not run on a sequencer yet: that needs a second program to play the owner.

## Initialization window

There is none with the deployer gate. The program id is the loader header account and the deployer holds its key, so between deployment and `Init` nobody else passes the gate, and nothing else is accepted on the empty config. A stranger's `Init` was refused with E117 on a local dev sequencer and the config read back empty. With a genesis key, that key alone passes.

What remains is the header key itself. A program deployed mutable can have its code swapped by that key, which no check inside the program can see. Deploy immutable, as the runner does.

## Renounce is terminal

`Renounce` writes `AuthoritySlot::Renounced` and that is the end. There is no recover and no re-init. A program that needs a pause or a freeze implements that as its own state, under the admin while there is one. An admin that loses its key leaves the program in the same state, reached by accident, and there is no recovery, because any recovery path would also be an exploit path.
