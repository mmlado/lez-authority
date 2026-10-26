//! The README's program: one role, one gated value, no targets.

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
