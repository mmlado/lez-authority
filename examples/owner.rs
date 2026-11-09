//! The README's program: a plain LEZ program with one role and one gated value.

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
