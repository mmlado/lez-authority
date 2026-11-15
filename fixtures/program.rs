use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::program::{PdaSeed, ReceiveInput, Response};
use role::{AuthoritySlot, Gated, HasSlot, Sample};

const SEED: PdaSeed = role::lez_authority::seed(b"program-fixture/v1");

/// The program's config, with the role's slot.
#[derive(Default, BorshSerialize, BorshDeserialize, HasSlot)]
pub struct Config {
    #[slot(Sample)]
    sample: AuthoritySlot,
}

/// The program's messages.
#[derive(BorshSerialize, BorshDeserialize, Gated)]
pub enum Message {
    /// Needs the role's holder.
    #[only(Sample)]
    Set,
}

/// The program's handler, behind the guard.
#[role::guard(roles(Sample), config = Config, seed = SEED)]
pub fn handle(_input: &ReceiveInput, _message: Message) -> Response {
    Response::keep_state()
}

#[cfg(test)]
mod tests {
    use lee_core::account::{AccountId, Actor, ActorState};

    use super::*;

    #[test]
    #[should_panic(expected = "lez-authority/sample E104: the sender must authorize the change")]
    fn the_guard_runs_through_the_role_crate() {
        let message = role::Wire::from(Message::Set);
        let input = ReceiveInput {
            receiver: Actor::new(AccountId::new([1; 32]), AccountId::new([7; 32])),
            from: None,
            is_authorized: false,
            pre_state: ActorState::from(Vec::new()),
            message: borsh::to_vec(&message).unwrap(),
        };
        let _ = handle(&input, message);
    }
}
