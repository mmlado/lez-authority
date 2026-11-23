# Migrating from spel-admin-authority v0.2

For a program on [spel-admin-authority](https://github.com/mmlado/spel-admin-authority) v0.2.x moving to LEZ 0.4. Nothing migrates in place: LEZ 0.4 replaces instructions and account lists with actors and messages, programs are redeployed, and a program's state starts empty on the new chain. What carries over is the model, and most of the names.

## What changed underneath

| LEZ v0.2 | LEZ 0.4 |
| --- | --- |
| A program exposes instructions, each with an account list | A program is one handler that runs at an actor, the pair of an account and the program, and receives messages |
| Every account in the transaction comes with `is_authorized` | Only the receiving account's authorization is visible, at its own actor |
| Instructions are dispatched by SPEL from the IDL | The program decodes its own message enum, and there is no IDL yet |
| Programs call each other by chained calls with account lists | Actors send each other sync and async messages, and a PDA's seed travels as a grant on a sync message |
| Deployment is bytecode only, and the first caller of `admin_initialize` self-elects | The program id is the loader header account, whose key the deployer holds |
| Errors are `SpelError::Custom` with a code the client sees | A refusal is a panic. The wallet returns its text as `SimulatedRevert` before sending, and a transaction sent anyway is reverted with the text in the sequencer log |

## Concept map

| v0.2 | 0.4 | Note |
| --- | --- | --- |
| `#[admin_authority]` on the module | `#[lez_admin_authority::admin(config = .., seed = ..)]` on your handler, `#[derive(HasSlot)]` on your config, `#[derive(Gated)]` on your message enum | the handler stays a plain LEZ handler |
| `#[require_admin]` on an instruction | `#[only(Admin)]` on the message | the guard checks it before the handler |
| `#[admin_slot]` on the embedded field | `#[slot(Admin)]` on the field | embedded is the only mode |
| `AdminConfig`, 64 bytes, admin then nominee | `AuthoritySlot`, a Borsh enum: unset, held with its nominee, renounced | the holder carries its `via` |
| The Config PDA at `(program_id, "admin_config")` | Your config account at `(program, your seed)`, holding your whole config | one account, your layout |
| Embedded mode with `offset` | The default and only shape | no offset, Borsh finds the field |
| `admin_initialize`, self-election | `AdminOp::Init` from the deployer's seat | the init gate closes the race |
| `SPEL_ADMIN_GENESIS` at build time | `genesis = KEY` on the attribute | compiled in, part of the program id |
| `admin_transfer` with `AdminCandidate` and a co-signer | gone, use nominate and accept | a PDA cannot co-sign |
| `admin_nominate`, `admin_accept` | `AdminOp::Nominate { next }`, `AdminOp::Accept { pda_seed }` | the PDA accept carries its seed |
| `admin_renounce` | `AdminOp::Renounce` | terminal, as before |
| Five injected instructions in the IDL | Four library messages a client builds with `admin_op` | they start with the byte 255, your enum hosts none of them |
| `AdminError`, `6000 + code` on the client | `AuthorityError`, `lez-authority/admin E<code>: <text>` in the wallet's `SimulatedRevert` and in the sequencer log | numbers kept where the meaning is the same |
| `caller-pda-seeds` on a chained call | `pda_seeds` on the owning program's sync message | the seat sees the grant as authorization |
| `UnownedSigner`, `wallet auth-transfer init` | no counterpart | fresh keys signed repeatedly on the dev stack |
| `spel` CLI, `--co-signer`, `--export` | no CLI yet, the LEZ wallet library and the runner | a client crate is issue #1 |

## Side by side

A gated instruction in v0.2:

```rust
#[instruction]
#[require_admin]
pub fn update_value(
    #[account(pda = literal("admin_config"))] admin_config: AccountWithMetadata,
    #[account(signer)] caller: AccountWithMetadata,
    #[account(mut, pda = literal("program_config"))] mut config: AccountWithMetadata,
    new_value: u64,
) -> SpelResult {
    // the admin check ran before this
}
```

The same in 0.4, one marked variant of the message enum and one arm of the plain handler:

```rust
#[only(Admin)]
UpdateValue(u64),

Message::UpdateValue(new_value) => {
    let mut config: Config = borsh::from_slice(&input.pre_state).expect("the config decodes");
    config.value = new_value;
    Response::set_state(borsh::to_vec(&config).expect("the config encodes"))
}
```

The accounts are gone from the signature: the config is the state of the actor the handler runs at, and the caller is what the guard knows from the hop. The client, instead of listing `admin_config`, `caller` and `config`, lists the admin's seat and the config account as the actors of one transaction.

## Error codes

Numbers up to 116 follow the v0.2 table where a refusal has the same meaning. The rest of the v0.2 table is retired, and new refusals start at 117.

| Code | v0.2 | 0.4 |
| --- | --- | --- |
| 101 | `NotInitialized` | `NotInitialized` |
| 102 | `Renounced` | `Renounced` |
| 103 | `NotAdmin` | `NotHolder` |
| 104 | `MissingSignature` | `MissingSignature` |
| 105 | `InvalidCandidate` | retired, there is no candidate |
| 106 | `UndeployedPda` | retired, a PDA proves itself by accepting through its program |
| 107 | `CandidateMismatch` | `SeedProofFailed` |
| 108 | `EncodingFailed` | `MessageDoesNotDecode` |
| 109 | `DecodingFailed` | `ConfigDoesNotDecode` |
| 110 | `AccountDataTooLarge` | retired |
| 111 | `SlotOutOfBounds` | retired, there is no offset |
| 112 | `UnownedSigner` | retired |
| 113 | `NoNomination` | `NoNomination` |
| 114 | `NotNominee` | `NotNominee` |
| 115 | `NotGenesisAdmin` | `NotGenesis` |
| 116 | `ConfigNotOwned` | retired, the config account is the program's own actor |
| 117 to 135 | | new, the routing refusals: see the [table](https://github.com/mmlado/lez-authority#refusals) |

## Steps

1. **One config struct.** Collect what the program kept on its accounts into one Borsh struct with `Default`, and add the `AuthoritySlot` field marked `#[slot(Admin)]`, deriving `HasSlot`. State that belongs to other accounts, such as a per-user value, stays on those accounts, written by messages the config sends.
2. **One message enum.** One variant per instruction, with the instruction's arguments as fields and no accounts. Derive `Gated`, mark each message whose instruction had `#[require_admin]` with `#[only(Admin)]`, and each message the config sends to another account with `#[from_config]`.
3. **The handler.** A plain LEZ handler, `fn(&ReceiveInput, Message) -> Response`, one `match` arm per variant. Marked messages reach it already checked. Where the instruction wrote another account, the arm's response sends a `#[from_config]` message to that account's actor, and the arm for that message writes it there.
4. **The entry.** `#[lez_admin_authority::admin(config = Config, seed = CONFIG_SEED)]` on the handler, then `lee_core::actor_entrypoint!(handle_message)`.
5. **Drop SPEL.** `spel-framework`, `admin-authority` and the macros go. The dependencies are `borsh`, `lee_core` at the pinned commit, and `lez-admin-authority`.
6. **Build the guest** through a methods crate with `risc0_build::embed_methods`, as `sample/methods` does, with the risc0 toolchain at 3.0.5.
7. **Deploy immutable and send `Init`** from the deployer's seat, as `admin_op(AdminOp::Init)`. Then nominate the admin the program should have, and have it accept.
8. **Clients.** Replace IDL-driven calls with transactions from the LEZ wallet library: the root actor, the actors reached, the signer, and the message: Borsh for your enum, `admin_op` for the admin's operations. The README's transaction shapes and the runner show each one. Read the config back after each call. A refused call is not sent by default: the wallet returns `SimulatedRevert` with the refusal text. Sent anyway, it is a reverted, charged transaction with its text in the sequencer log.

## Not there yet

A one-step transfer, a CLI and an IDL, the SPEL shim, and the freeze role. See the README's [Not in this release](../README.md#not-in-this-release).
