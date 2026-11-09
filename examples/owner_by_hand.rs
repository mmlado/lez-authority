//! The README's program without the macros: the impls the derives write, and the guard the
//! attribute puts in front of the handler.

use std::any::TypeId;

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::program::{PdaSeed, ReceiveInput, Response};
use lez_authority::{AuthoritySlot, Gated, HasSlot, InitGate, Policy, Role, Wire};

/// The role this program has one holder of.
pub struct Owner;

impl Role for Owner {
    const NAME: &'static str = "owner";
}

/// The owner answers to nobody, so the default rules hold.
impl<C: HasSlot<Owner>> Policy<C> for Owner {}

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
}

impl Gated for Message {
    fn needs<R: Role>(&self) -> bool {
        match self {
            Message::SetValue(_) => TypeId::of::<R>() == TypeId::of::<Owner>(),
        }
    }
}

fn handle_message(input: &ReceiveInput, message: Wire<Message>) -> Response {
    fn handle_message(input: &ReceiveInput, message: Message) -> Response {
        let mut config: Config = borsh::from_slice(&input.pre_state).expect("the config decodes");
        match message {
            Message::SetValue(value) => config.value = value,
        }
        Response::set_state(borsh::to_vec(&config).expect("the config encodes"))
    }
    lez_authority::guard::<(Owner,), Config, Message>(
        input,
        &CONFIG_SEED,
        InitGate::Deployer,
        message,
        handle_message,
    )
}

lee_core::actor_entrypoint!(handle_message);
