//! The smallest program with an admin and a freeze authority: a plain LEZ program whose state
//! lives on one account. It depends on lez-freeze-authority alone, keeps the freeze slot beside
//! the admin's in its config, names both roles in the engine's guard attribute, and marks the
//! one message that runs through a freeze.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::program::{PdaSeed, ReceiveInput, Response};
use lez_freeze_authority::{Admin, AuthoritySlot, Freeze, FreezeSlot, Gated, HasSlot};

const CONFIG_SEED: PdaSeed = lez_freeze_authority::seed(b"lez-freeze-sample-minimal/v1");

#[derive(Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, HasSlot)]
struct Config {
    open_value: u64,
    admin_value: u64,
    exempt_value: u64,
    #[slot(Admin)]
    admin: AuthoritySlot,
    #[slot(Freeze)]
    freeze: FreezeSlot,
}

#[allow(clippy::enum_variant_names)]
#[derive(Debug, BorshSerialize, BorshDeserialize, Gated)]
enum Message {
    SetOpenValue(u64),
    #[only(Admin)]
    SetAdminValue(u64),
    #[exempt(Freeze)]
    SetExemptValue(u64),
}

#[lez_freeze_authority::guard(roles(Admin, Freeze), config = Config, seed = CONFIG_SEED)]
fn handle_message(input: &ReceiveInput, message: Message) -> Response {
    let mut config: Config = borsh::from_slice(&input.pre_state).expect("the config decodes");
    match message {
        Message::SetOpenValue(value) => config.open_value = value,
        Message::SetAdminValue(value) => config.admin_value = value,
        Message::SetExemptValue(value) => config.exempt_value = value,
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
    use lez_freeze_authority::{
        FreezeOp, Holder, Op, Slot, Wire, admin_op, freeze_authority_op, freeze_op,
    };

    use super::*;

    const PROGRAM: AccountId = AccountId::new([0xB1; 32]);
    const DEPLOYER: AccountId = PROGRAM;
    const AUTHORITY: AccountId = AccountId::new([1; 32]);
    const SUCCESSOR: AccountId = AccountId::new([2; 32]);
    const STRANGER: AccountId = AccountId::new([3; 32]);

    fn config_actor() -> Actor {
        Actor::new(
            lez_freeze_authority::config_account(PROGRAM, &CONFIG_SEED),
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

    fn send(sender: AccountId, pre: &Config, message: Wire<Message>) -> Config {
        written(&through_seat(sender, borsh::to_vec(pre).unwrap(), message))
    }

    fn holder(account: AccountId) -> AuthoritySlot {
        AuthoritySlot::held(Holder { account, via: None })
    }

    /// The config once the deployer initialized it, with `freeze` as the freeze slot's holder
    /// part.
    fn config(freeze: AuthoritySlot) -> Config {
        let mut config = Config {
            admin: holder(DEPLOYER),
            ..Config::default()
        };
        *config.freeze.authority_slot_mut() = freeze;
        config
    }

    fn nominate(next: AccountId) -> Wire<Message> {
        freeze_authority_op(Op::Nominate { next })
    }

    fn accept() -> Wire<Message> {
        freeze_authority_op(Op::Accept { pda_seed: None })
    }

    /// `message` sent straight to the config, by nobody in particular.
    fn straight(pre: &Config, message: Message) -> Config {
        written(&run(
            config_actor(),
            None,
            false,
            borsh::to_vec(pre).unwrap(),
            message.into(),
        ))
    }

    /// The config once the freeze authority froze the program.
    fn frozen_program() -> Config {
        send(
            AUTHORITY,
            &config(holder(AUTHORITY)),
            freeze_op(FreezeOp::Freeze),
        )
    }

    #[test]
    fn the_deployer_initializes_and_the_freeze_slot_is_born_unset() {
        let init = written(&through_seat(DEPLOYER, Vec::new(), admin_op(Op::Init)));
        assert_eq!(init, config(AuthoritySlot::Unset));
        assert_eq!(init.freeze, FreezeSlot::default());
    }

    #[test]
    fn anyone_sets_the_open_value_only_the_admin_the_other() {
        let appointed = config(holder(AUTHORITY));
        let open = run(
            config_actor(),
            None,
            false,
            borsh::to_vec(&appointed).unwrap(),
            Message::SetOpenValue(7).into(),
        );
        assert_eq!(
            written(&open),
            Config {
                open_value: 7,
                ..config(holder(AUTHORITY))
            }
        );
        assert_eq!(
            send(DEPLOYER, &appointed, Message::SetAdminValue(9).into()),
            Config {
                admin_value: 9,
                ..config(holder(AUTHORITY))
            }
        );
    }

    #[test]
    fn the_admin_appoints_replaces_revokes_and_appoints_again() {
        let offered = send(DEPLOYER, &config(AuthoritySlot::Unset), nominate(AUTHORITY));
        let appointed = send(AUTHORITY, &offered, accept());
        assert_eq!(appointed, config(holder(AUTHORITY)));
        let nominated = send(DEPLOYER, &appointed, nominate(SUCCESSOR));
        let replaced = send(SUCCESSOR, &nominated, accept());
        assert_eq!(replaced, config(holder(SUCCESSOR)));
        let revoked = send(DEPLOYER, &replaced, freeze_authority_op(Op::Renounce));
        assert_eq!(revoked, config(AuthoritySlot::Unset));
        let offered = send(DEPLOYER, &revoked, nominate(AUTHORITY));
        assert_eq!(
            send(AUTHORITY, &offered, accept()),
            config(holder(AUTHORITY))
        );
    }

    #[test]
    fn the_freeze_authority_hands_its_role_on_after_the_admin_renounced() {
        let without_admin = send(DEPLOYER, &config(holder(AUTHORITY)), admin_op(Op::Renounce));
        assert_eq!(without_admin.admin, AuthoritySlot::Renounced);
        let nominated = send(AUTHORITY, &without_admin, nominate(SUCCESSOR));
        let handed_on = send(SUCCESSOR, &nominated, accept());
        assert_eq!(handed_on.freeze.authority_slot(), &holder(SUCCESSOR));
    }

    #[test]
    #[should_panic(
        expected = "lez-authority/freeze E137: the freeze authority is appointed by nomination, never installed"
    )]
    fn the_freeze_role_has_no_init() {
        send(
            DEPLOYER,
            &config(AuthoritySlot::Unset),
            freeze_authority_op(Op::Init),
        );
    }

    #[test]
    #[should_panic(
        expected = "lez-freeze-authority/freeze E208: the signing account is neither the freeze authority nor the admin"
    )]
    fn a_stranger_cannot_appoint() {
        send(STRANGER, &config(AuthoritySlot::Unset), nominate(STRANGER));
    }

    #[test]
    #[should_panic(
        expected = "lez-authority/freeze E114: the signing account is not the nominated freeze authority"
    )]
    fn only_the_nominee_accepts() {
        send(
            STRANGER,
            &config(AuthoritySlot::Offered { nominee: AUTHORITY }),
            accept(),
        );
    }

    #[test]
    #[should_panic(expected = "lez-authority/admin E103: the signing account is not the admin")]
    fn a_stranger_is_refused_the_admin_message() {
        send(
            STRANGER,
            &config(AuthoritySlot::Unset),
            Message::SetAdminValue(1).into(),
        );
    }

    #[test]
    #[should_panic(expected = "lez-authority/admin E102: the admin is renounced")]
    fn once_the_admin_is_renounced_its_message_is_refused() {
        let without_admin = Config {
            admin: AuthoritySlot::Renounced,
            ..config(holder(AUTHORITY))
        };
        send(DEPLOYER, &without_admin, Message::SetAdminValue(1).into());
    }

    #[test]
    fn the_freeze_authority_freezes_and_releases_the_program() {
        let frozen = frozen_program();
        assert!(frozen.freeze.is_frozen());
        assert_eq!(
            send(AUTHORITY, &frozen, freeze_op(FreezeOp::Release)),
            config(holder(AUTHORITY))
        );
    }

    #[test]
    #[should_panic(expected = "lez-freeze-authority/freeze E217: the program is frozen")]
    fn a_freeze_refuses_the_open_message() {
        straight(&frozen_program(), Message::SetOpenValue(7));
    }

    #[test]
    #[should_panic(expected = "lez-freeze-authority/freeze E217: the program is frozen")]
    fn a_freeze_refuses_the_admin_message() {
        send(
            DEPLOYER,
            &frozen_program(),
            Message::SetAdminValue(9).into(),
        );
    }

    #[test]
    fn anyone_sets_the_exempt_value_through_a_freeze() {
        assert_eq!(
            straight(&frozen_program(), Message::SetExemptValue(5)),
            Config {
                exempt_value: 5,
                ..frozen_program()
            }
        );
    }

    #[test]
    fn after_a_release_every_message_runs_again() {
        let released = send(AUTHORITY, &frozen_program(), freeze_op(FreezeOp::Release));
        let opened = straight(&released, Message::SetOpenValue(7));
        assert_eq!(
            send(DEPLOYER, &opened, Message::SetAdminValue(9).into()),
            Config {
                open_value: 7,
                admin_value: 9,
                ..config(holder(AUTHORITY))
            }
        );
    }

    #[test]
    fn the_admin_replaces_the_freeze_authority_of_a_frozen_program() {
        let nominated = send(DEPLOYER, &frozen_program(), nominate(SUCCESSOR));
        let replaced = send(SUCCESSOR, &nominated, accept());
        assert!(replaced.freeze.is_frozen());
        assert_eq!(replaced.freeze.authority_slot(), &holder(SUCCESSOR));
        assert_eq!(
            send(SUCCESSOR, &replaced, freeze_op(FreezeOp::Release)),
            config(holder(SUCCESSOR))
        );
    }

    #[test]
    #[should_panic(
        expected = "lez-authority/freeze E103: the signing account is not the freeze authority"
    )]
    fn the_admin_cannot_freeze() {
        send(
            DEPLOYER,
            &config(holder(AUTHORITY)),
            freeze_op(FreezeOp::Freeze),
        );
    }
}
