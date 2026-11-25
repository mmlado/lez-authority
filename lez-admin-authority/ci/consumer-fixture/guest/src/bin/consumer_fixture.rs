//! CI consumer fixture: a downstream program on the published library, built as a real risc0 guest.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::program::{PdaSeed, ReceiveInput, Response};
use lez_admin_authority::{Admin, AuthoritySlot, Gated, HasSlot};

const CONFIG_SEED: PdaSeed = lez_admin_authority::seed(b"lez-admin-consumer-fixture/v1");

#[derive(Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, HasSlot)]
struct Config {
    open_value: u64,
    admin_value: u64,
    #[slot(Admin)]
    admin: AuthoritySlot,
}

#[derive(Debug, BorshSerialize, BorshDeserialize, Gated)]
enum Message {
    SetOpenValue(u64),
    #[only(Admin)]
    SetAdminValue(u64),
}

#[lez_admin_authority::admin(config = Config, seed = CONFIG_SEED)]
fn handle_message(input: &ReceiveInput, message: Message) -> Response {
    let mut config: Config = borsh::from_slice(&input.pre_state).expect("the config decodes");
    match message {
        Message::SetOpenValue(value) => config.open_value = value,
        Message::SetAdminValue(value) => config.admin_value = value,
    }
    Response::set_state(borsh::to_vec(&config).expect("the config encodes"))
}

lee_core::actor_entrypoint!(handle_message);

#[cfg(test)]
mod tests {
    use lee_core::{
        account::{AccountId, Actor, ActorState},
        program::{SyncMessage, Transition},
    };
    use lez_admin_authority::{AdminOp, Holder, Wire, admin_op};

    use super::*;

    const PROGRAM: AccountId = AccountId::new([0xB0; 32]);
    const DEPLOYER: AccountId = PROGRAM;
    const STRANGER: AccountId = AccountId::new([2; 32]);

    fn config_actor() -> Actor {
        Actor::new(
            lez_admin_authority::config_account(PROGRAM, &CONFIG_SEED),
            PROGRAM,
        )
    }

    fn run(
        receiver: Actor,
        from: Option<Actor>,
        authorized: bool,
        pre: Vec<u8>,
        message: Wire<Message>,
    ) -> Transition {
        let input = ReceiveInput {
            receiver,
            from,
            is_authorized: authorized,
            pre_state: ActorState::from(pre),
            message: borsh::to_vec(&message).unwrap(),
        };
        handle_message(&input, message).into_transition(input)
    }

    /// The sender's own actor forwards, then the config handles what it forwarded.
    fn through_seat(sender: AccountId, pre: Vec<u8>, message: Wire<Message>) -> Transition {
        let hop = run(Actor::new(sender, PROGRAM), None, true, Vec::new(), message);
        let [SyncMessage { message, .. }] =
            <[SyncMessage; 1]>::try_from(hop.response.sync_messages).unwrap();
        run(
            config_actor(),
            Some(Actor::new(sender, PROGRAM)),
            false,
            pre,
            borsh::from_slice(&message).unwrap(),
        )
    }

    fn written(transition: &Transition) -> Config {
        borsh::from_slice(transition.response.post_state.as_ref().unwrap()).unwrap()
    }

    fn initialized() -> Config {
        Config {
            open_value: 0,
            admin_value: 0,
            admin: AuthoritySlot::held(Holder {
                account: DEPLOYER,
                via: None,
            }),
        }
    }

    #[test]
    fn the_deployer_initializes_anyone_sets_the_open_value_only_the_admin_the_other() {
        let init = through_seat(DEPLOYER, Vec::new(), admin_op(AdminOp::Init));
        assert_eq!(written(&init), initialized());

        let open = run(
            config_actor(),
            None,
            false,
            borsh::to_vec(&initialized()).unwrap(),
            Message::SetOpenValue(7).into(),
        );
        assert_eq!(written(&open).open_value, 7);

        let admin = through_seat(
            DEPLOYER,
            borsh::to_vec(&initialized()).unwrap(),
            Message::SetAdminValue(9).into(),
        );
        assert_eq!(written(&admin).admin_value, 9);
    }

    #[test]
    #[should_panic(expected = "E103: the signing account is not the admin")]
    fn a_stranger_is_refused() {
        let _transition = through_seat(
            STRANGER,
            borsh::to_vec(&initialized()).unwrap(),
            Message::SetAdminValue(1).into(),
        );
    }
}
