//! Sample program, written as a plain LEZ program: two values on the config, two setters, one
//! of them for the admin only, and a per-account value the admin sets on another actor of the
//! program.
//!
//! The config lives on the config account, a PDA of the program id and `CONFIG_SEED`. The
//! attribute puts the guard in front of the handler. A message marked `#[only(Admin)]` goes to
//! the sender's own actor first, which vouches for the sender and forwards it to the config,
//! where the guard checks the admin before the handler runs. The admin's operations never
//! reach the handler. Only the deployer, the program's own account, sends `Init`, because the
//! program id is the loader header account and the deployer holds its key. Until then every
//! message at the config is refused. The first write to the config account needs
//! `PublicAccountEvidence::Pda { program, seed }` in the transaction.

use admin_sample_core::{CONFIG_SEED, Config, Message, TargetState};
use borsh::BorshDeserialize;
use lee_core::account::Actor;
use lee_core::program::{ReceiveInput, Response};

#[lez_admin_authority::admin(config = Config, seed = CONFIG_SEED)]
fn handle_message(input: &ReceiveInput, message: Message) -> Response {
    match message {
        Message::SetOpenValue(value) => {
            let mut config: Config = state(input);
            config.open_value = value;
            Response::set_state(borsh::to_vec(&config).expect("the config encodes"))
        }
        Message::SetAdminValue(value) => {
            let mut config: Config = state(input);
            config.admin_value = value;
            Response::set_state(borsh::to_vec(&config).expect("the config encodes"))
        }
        Message::SetTargetValue { target, value } => {
            let target = Actor::new(target, input.receiver.program_account_id);
            Response::keep_state().send_sync(target, &Message::TargetValue(value))
        }
        Message::TargetValue(value) => {
            let mut target: TargetState = state(input);
            target.value = value;
            Response::set_state(borsh::to_vec(&target).expect("the target state encodes"))
        }
    }
}

/// The actor's state, or the default on an account that holds none yet.
fn state<T: Default + BorshDeserialize>(input: &ReceiveInput) -> T {
    let pre: &[u8] = &input.pre_state;
    if pre.is_empty() {
        T::default()
    } else {
        borsh::from_slice(pre).expect("the state decodes")
    }
}

lee_core::actor_entrypoint!(handle_message);

#[cfg(test)]
mod tests {
    use admin_sample_core::config_account;
    use lee_core::{
        account::{AccountId, Actor, ActorState},
        program::{SyncMessage, Transition},
    };
    use lez_admin_authority::lez_authority::{Forwarded, LibraryMessage};
    use lez_admin_authority::{AdminOp, AuthoritySlot, Holder, InitGate, Wire, admin_op};

    use super::*;

    const PROGRAM: AccountId = AccountId::new([0xA0; 32]);
    // The deployer is the program's own account: the header account whose key deployed it.
    const DEPLOYER: AccountId = PROGRAM;
    const GENESIS: AccountId = AccountId::new([1; 32]);
    const STRANGER: AccountId = AccountId::new([2; 32]);
    const GOVERNANCE: AccountId = AccountId::new([6; 32]);

    fn actor(account: AccountId) -> Actor {
        Actor::new(account, PROGRAM)
    }

    fn config_actor() -> Actor {
        actor(config_account(PROGRAM))
    }

    fn held(account: AccountId, via: Option<AccountId>) -> AuthoritySlot {
        AuthoritySlot::held(Holder { account, via })
    }

    fn held_with_nominee(nominee: AccountId) -> AuthoritySlot {
        AuthoritySlot::Held {
            holder: Holder {
                account: DEPLOYER,
                via: None,
            },
            nominee: Some(nominee),
        }
    }

    fn nominated() -> Config {
        Config {
            admin: held_with_nominee(STRANGER),
            ..initialized()
        }
    }

    fn initialized() -> Config {
        Config {
            open_value: 0,
            admin_value: 0,
            admin: held(DEPLOYER, None),
        }
    }

    fn bytes(config: &Config) -> Vec<u8> {
        borsh::to_vec(config).unwrap()
    }

    fn input(
        receiver: Actor,
        from: Option<Actor>,
        is_authorized: bool,
        pre: Vec<u8>,
        message: &Wire<Message>,
    ) -> ReceiveInput {
        ReceiveInput {
            receiver,
            from,
            is_authorized,
            pre_state: ActorState::from(pre),
            message: borsh::to_vec(message).unwrap(),
        }
    }

    fn run(
        receiver: Actor,
        from: Option<Actor>,
        is_authorized: bool,
        pre: Vec<u8>,
        message: Wire<Message>,
    ) -> Transition {
        let input = input(receiver, from, is_authorized, pre, &message);
        handle_message(&input, message).into_transition(input)
    }

    fn at_config_from(from: Option<Actor>, pre: Vec<u8>, message: Wire<Message>) -> Transition {
        run(config_actor(), from, false, pre, message)
    }

    /// Hop 1 at the sender's own actor, then hop 2 at the config with what it forwarded.
    fn through_seat(
        seat: AccountId,
        from: Option<Actor>,
        is_authorized: bool,
        pre: Vec<u8>,
        message: Wire<Message>,
    ) -> Transition {
        let hop = run(actor(seat), from, is_authorized, Vec::new(), message);
        assert!(
            hop.response.post_state.is_none() && hop.response.async_messages.is_empty(),
            "the seat only forwards"
        );
        let [SyncMessage { to, message, .. }] =
            <[SyncMessage; 1]>::try_from(hop.response.sync_messages)
                .expect("one forwarded message");
        assert_eq!(to, config_actor());
        at_config_from(
            Some(actor(seat)),
            pre,
            borsh::from_slice(&message).expect("the forwarded message decodes"),
        )
    }

    fn written(transition: &Transition) -> Config {
        let state = transition
            .response
            .post_state
            .as_ref()
            .expect("the config is written");
        borsh::from_slice(state).expect("the config decodes")
    }

    fn forwarded(via: Option<AccountId>, message: &Wire<Message>) -> Wire<Message> {
        Wire::Library(LibraryMessage::Forwarded(Forwarded {
            via,
            inner: borsh::to_vec(message).unwrap(),
        }))
    }

    #[test]
    fn init_through_the_seat_installs_the_sender_as_admin() {
        let transition = through_seat(DEPLOYER, None, true, Vec::new(), admin_op(AdminOp::Init));
        assert_eq!(written(&transition), initialized());
    }

    #[test]
    fn the_seat_forwards_naming_the_program_that_reached_it() {
        let transition = run(
            actor(DEPLOYER),
            None,
            true,
            Vec::new(),
            admin_op(AdminOp::Init),
        );
        assert_eq!(
            transition.response.sync_messages,
            vec![SyncMessage::new(
                config_actor(),
                &forwarded(None, &admin_op(AdminOp::Init))
            )]
        );
    }

    #[test]
    #[should_panic(expected = "the sender must authorize the change")]
    fn an_unsigned_sender_is_refused_at_its_seat() {
        let _transition = run(
            actor(DEPLOYER),
            None,
            false,
            Vec::new(),
            admin_op(AdminOp::Init),
        );
    }

    #[test]
    #[should_panic(expected = "must go through the sender's own actor")]
    fn init_sent_straight_to_the_config_is_refused() {
        let _transition = at_config_from(None, Vec::new(), admin_op(AdminOp::Init));
    }

    #[test]
    #[should_panic(expected = "only comes from this program's own actor")]
    fn a_forged_forward_sent_straight_to_the_config_is_refused() {
        let _transition =
            at_config_from(None, Vec::new(), forwarded(None, &admin_op(AdminOp::Init)));
    }

    #[test]
    #[should_panic(expected = "forwarded messages are only handled at the config account")]
    fn a_forward_sent_to_a_seat_is_refused() {
        let message = forwarded(None, &admin_op(AdminOp::Init));
        let _transition = run(actor(DEPLOYER), None, true, Vec::new(), message);
    }

    #[test]
    #[should_panic(expected = "only the deployer installs the first admin")]
    fn init_by_another_account_is_refused() {
        let _transition = through_seat(STRANGER, None, true, Vec::new(), admin_op(AdminOp::Init));
    }

    #[test]
    #[should_panic(expected = "only the deployer installs the first admin")]
    fn init_by_the_deployer_through_a_program_is_refused() {
        let governance = Actor::new(STRANGER, GOVERNANCE);
        let _transition = through_seat(
            DEPLOYER,
            Some(governance),
            true,
            Vec::new(),
            admin_op(AdminOp::Init),
        );
    }

    /// The same program with a genesis key. Its handler never runs for an operation.
    #[lez_admin_authority::admin(config = Config, seed = CONFIG_SEED, genesis = GENESIS)]
    fn handle_with_genesis(_input: &ReceiveInput, _message: Message) -> Response {
        Response::keep_state()
    }

    fn through_seat_with_genesis(
        seat: AccountId,
        pre: Vec<u8>,
        message: Wire<Message>,
    ) -> Transition {
        let run = |receiver: Actor, from: Option<Actor>, pre: Vec<u8>, message: Wire<Message>| {
            let input = input(receiver, from, true, pre, &message);
            handle_with_genesis(&input, message).into_transition(input)
        };
        let hop = run(actor(seat), None, Vec::new(), message);
        let [SyncMessage { message, .. }] =
            <[SyncMessage; 1]>::try_from(hop.response.sync_messages)
                .expect("one forwarded message");
        run(
            config_actor(),
            Some(actor(seat)),
            pre,
            borsh::from_slice(&message).unwrap(),
        )
    }

    #[test]
    fn with_a_genesis_key_that_key_installs_itself() {
        let transition = through_seat_with_genesis(GENESIS, Vec::new(), admin_op(AdminOp::Init));
        assert_eq!(written(&transition).admin, held(GENESIS, None));
    }

    #[test]
    #[should_panic(expected = "only the genesis key installs the first admin")]
    fn with_a_genesis_key_the_deployer_is_refused() {
        let _transition = through_seat_with_genesis(DEPLOYER, Vec::new(), admin_op(AdminOp::Init));
    }

    #[test]
    #[should_panic(expected = "the admin is already installed")]
    fn a_replayed_init_is_refused_too() {
        let _transition = through_seat(
            DEPLOYER,
            None,
            true,
            bytes(&initialized()),
            admin_op(AdminOp::Init),
        );
    }

    #[test]
    fn anyone_sets_the_open_value() {
        let transition =
            at_config_from(None, bytes(&initialized()), Message::SetOpenValue(7).into());
        let expected = Config {
            open_value: 7,
            ..initialized()
        };
        assert_eq!(written(&transition), expected);
    }

    #[test]
    #[should_panic(expected = "the config account is not initialized")]
    fn before_init_even_an_open_message_is_refused() {
        let _transition = at_config_from(None, Vec::new(), Message::SetOpenValue(7).into());
    }

    #[test]
    fn an_open_message_sent_to_a_seat_runs_there() {
        let transition = run(
            actor(STRANGER),
            None,
            true,
            Vec::new(),
            Message::SetOpenValue(3).into(),
        );
        assert!(
            transition.response.sync_messages.is_empty(),
            "nothing is forwarded"
        );
        assert_eq!(written(&transition).open_value, 3);
    }

    #[test]
    #[should_panic(expected = "the config account is not initialized")]
    fn before_init_the_admin_value_is_locked() {
        let _transition = through_seat(
            DEPLOYER,
            None,
            true,
            Vec::new(),
            Message::SetAdminValue(1).into(),
        );
    }

    #[test]
    fn the_admin_sets_the_admin_value() {
        let transition = through_seat(
            DEPLOYER,
            None,
            true,
            bytes(&initialized()),
            Message::SetAdminValue(9).into(),
        );
        let expected = Config {
            admin_value: 9,
            ..initialized()
        };
        assert_eq!(written(&transition), expected);
    }

    #[test]
    #[should_panic(expected = "the signing account is not the admin")]
    fn a_signed_stranger_cannot_set_the_admin_value() {
        let _transition = through_seat(
            STRANGER,
            None,
            true,
            bytes(&initialized()),
            Message::SetAdminValue(9).into(),
        );
    }

    #[test]
    #[should_panic(expected = "must go through the sender's own actor")]
    fn an_admin_message_sent_straight_to_the_config_is_refused() {
        let _transition = at_config_from(
            None,
            bytes(&initialized()),
            Message::SetAdminValue(9).into(),
        );
    }

    // The admin signed a transaction rooted in some other program, which then reached the
    // admin's seat. The seat is authorized, but the config refuses the program in the middle.
    #[test]
    #[should_panic(expected = "the admin acts at top level")]
    fn a_change_entered_from_another_program_is_refused() {
        let other = Actor::new(STRANGER, STRANGER);
        let _transition = through_seat(
            DEPLOYER,
            Some(other),
            true,
            bytes(&initialized()),
            Message::SetAdminValue(9).into(),
        );
    }

    // A PDA admin, which only accept can install, acts through its program and only there.
    #[test]
    fn a_pda_admin_acts_through_its_program() {
        let governance = Actor::new(STRANGER, GOVERNANCE);
        let pda_admin = Config {
            admin: held(DEPLOYER, Some(GOVERNANCE)),
            ..initialized()
        };
        let transition = through_seat(
            DEPLOYER,
            Some(governance),
            true,
            bytes(&pda_admin),
            Message::SetAdminValue(9).into(),
        );
        assert_eq!(written(&transition).admin_value, 9);
    }

    #[test]
    #[should_panic(expected = "the admin acts at top level")]
    fn a_pda_admin_cannot_act_at_top_level() {
        let pda_admin = Config {
            admin: held(DEPLOYER, Some(GOVERNANCE)),
            ..initialized()
        };
        let _transition = through_seat(
            DEPLOYER,
            None,
            true,
            bytes(&pda_admin),
            Message::SetAdminValue(9).into(),
        );
    }

    #[test]
    #[should_panic(expected = "the sender must authorize the change")]
    fn a_program_without_the_seed_is_refused_at_the_seat() {
        let governance = Actor::new(STRANGER, GOVERNANCE);
        let _transition = through_seat(
            DEPLOYER,
            Some(governance),
            false,
            bytes(&initialized()),
            Message::SetAdminValue(9).into(),
        );
    }

    #[test]
    fn renounce_empties_the_slot_and_keeps_the_values() {
        let pre = Config {
            open_value: 7,
            admin_value: 9,
            ..initialized()
        };
        let transition = through_seat(
            DEPLOYER,
            None,
            true,
            bytes(&pre),
            admin_op(AdminOp::Renounce),
        );
        let expected = Config {
            admin: AuthoritySlot::Renounced,
            ..pre
        };
        assert_eq!(written(&transition), expected);
    }

    #[test]
    #[should_panic(expected = "the admin is renounced")]
    fn a_renounced_admin_refuses_every_change() {
        let pre = Config {
            admin: AuthoritySlot::Renounced,
            ..initialized()
        };
        let _transition = through_seat(
            DEPLOYER,
            None,
            true,
            bytes(&pre),
            Message::SetAdminValue(1).into(),
        );
    }

    #[test]
    #[should_panic(expected = "the admin is renounced")]
    fn init_after_renounce_is_refused() {
        let pre = Config {
            admin: AuthoritySlot::Renounced,
            ..initialized()
        };
        let _transition = through_seat(DEPLOYER, None, true, bytes(&pre), admin_op(AdminOp::Init));
    }

    #[test]
    fn the_admin_nominates_and_nothing_moves_yet() {
        let message = admin_op(AdminOp::Nominate { next: STRANGER });
        let transition = through_seat(DEPLOYER, None, true, bytes(&initialized()), message);
        assert_eq!(written(&transition), nominated());

        let early = std::panic::catch_unwind(|| {
            through_seat(
                STRANGER,
                None,
                true,
                bytes(&nominated()),
                Message::SetAdminValue(1).into(),
            )
        });
        assert!(early.is_err(), "the nominee cannot act before accepting");
    }

    #[test]
    #[should_panic(expected = "the signing account is not the admin")]
    fn a_stranger_cannot_nominate() {
        let message = admin_op(AdminOp::Nominate { next: STRANGER });
        let _transition = through_seat(STRANGER, None, true, bytes(&initialized()), message);
    }

    #[test]
    #[should_panic(expected = "lez-authority/admin E108: the message does not decode")]
    fn an_operation_the_admin_does_not_define_is_refused() {
        let own = admin_op(AdminOp::Own(vec![0]));
        let _transition = through_seat(DEPLOYER, None, true, bytes(&initialized()), own);
    }

    #[test]
    fn the_nominee_accepts_and_the_old_admin_is_out() {
        let accept = admin_op(AdminOp::Accept { pda_seed: None });
        let transition = through_seat(STRANGER, None, true, bytes(&nominated()), accept);
        let handed_over = Config {
            admin: held(STRANGER, None),
            ..initialized()
        };
        assert_eq!(written(&transition), handed_over);

        let old = std::panic::catch_unwind(|| {
            through_seat(
                DEPLOYER,
                None,
                true,
                bytes(&handed_over),
                Message::SetAdminValue(2).into(),
            )
        });
        assert!(old.is_err(), "the old admin is refused after the handover");
        let new = through_seat(
            STRANGER,
            None,
            true,
            bytes(&handed_over),
            Message::SetAdminValue(3).into(),
        );
        assert_eq!(written(&new).admin_value, 3);
    }

    #[test]
    #[should_panic(expected = "no admin nominee is pending")]
    fn accept_without_a_nomination_is_refused() {
        let accept = admin_op(AdminOp::Accept { pda_seed: None });
        let _transition = through_seat(STRANGER, None, true, bytes(&initialized()), accept);
    }

    #[test]
    #[should_panic(expected = "the signing account is not the nominated admin")]
    fn only_the_nominee_accepts() {
        let other = AccountId::new([7; 32]);
        let accept = admin_op(AdminOp::Accept { pda_seed: None });
        let _transition = through_seat(other, None, true, bytes(&nominated()), accept);
    }

    #[test]
    fn a_pda_nominee_accepts_through_its_program_with_the_seed() {
        let seed = lez_admin_authority::seed(b"vault");
        let vault = AccountId::new(*b"xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx");
        let pda = AccountId::for_public_pda(&GOVERNANCE, &seed);
        let pre = Config {
            admin: held_with_nominee(pda),
            ..initialized()
        };
        let accept = admin_op(AdminOp::Accept {
            pda_seed: Some(seed),
        });
        let governance = Actor::new(vault, GOVERNANCE);
        let transition = through_seat(pda, Some(governance), true, bytes(&pre), accept);
        assert_eq!(written(&transition).admin, held(pda, Some(GOVERNANCE)));
    }

    #[test]
    #[should_panic(expected = "must prove itself with the seed")]
    fn a_pda_nominee_without_the_seed_is_refused() {
        let pda = AccountId::for_public_pda(&GOVERNANCE, &lez_admin_authority::seed(b"vault"));
        let pre = Config {
            admin: held_with_nominee(pda),
            ..initialized()
        };
        let accept = admin_op(AdminOp::Accept { pda_seed: None });
        let governance = Actor::new(STRANGER, GOVERNANCE);
        let _transition = through_seat(pda, Some(governance), true, bytes(&pre), accept);
    }

    #[test]
    fn a_nomination_can_be_cancelled_and_replaced() {
        let cancel = admin_op(AdminOp::Nominate {
            next: AccountId::default(),
        });
        let transition = through_seat(DEPLOYER, None, true, bytes(&nominated()), cancel);
        assert_eq!(written(&transition), initialized());

        let other = AccountId::new([7; 32]);
        let replace = admin_op(AdminOp::Nominate { next: other });
        let transition = through_seat(DEPLOYER, None, true, bytes(&nominated()), replace);
        assert_eq!(written(&transition).admin.nominee(), Some(other));
    }

    #[test]
    #[should_panic(expected = "the admin is renounced")]
    fn renounce_drops_the_nomination_too() {
        let transition = through_seat(
            DEPLOYER,
            None,
            true,
            bytes(&nominated()),
            admin_op(AdminOp::Renounce),
        );
        assert_eq!(written(&transition).admin, AuthoritySlot::Renounced);
        let accept = admin_op(AdminOp::Accept { pda_seed: None });
        let _transition = through_seat(STRANGER, None, true, bytes(&written(&transition)), accept);
    }

    #[test]
    fn the_admin_sets_a_target_value_through_the_hub() {
        let message = Message::SetTargetValue {
            target: STRANGER,
            value: 5,
        };
        let at_config = through_seat(DEPLOYER, None, true, bytes(&initialized()), message.into());
        assert!(
            at_config.response.post_state.is_none(),
            "the config itself is unchanged"
        );
        let expected = SyncMessage::new(actor(STRANGER), &Message::TargetValue(5));
        assert_eq!(at_config.response.sync_messages, vec![expected]);

        let at_target = run(
            actor(STRANGER),
            Some(config_actor()),
            false,
            Vec::new(),
            Message::TargetValue(5).into(),
        );
        let state: TargetState =
            borsh::from_slice(at_target.response.post_state.as_ref().unwrap()).unwrap();
        assert_eq!(state, TargetState { value: 5 });
    }

    #[test]
    #[should_panic(expected = "the signing account is not the admin")]
    fn a_stranger_cannot_reach_a_target() {
        let message = Message::SetTargetValue {
            target: STRANGER,
            value: 5,
        };
        let _transition = through_seat(STRANGER, None, true, bytes(&initialized()), message.into());
    }

    #[test]
    #[should_panic(expected = "this message comes only from the config account")]
    fn a_hub_message_sent_straight_to_a_target_is_refused() {
        let _transition = run(
            actor(STRANGER),
            None,
            false,
            Vec::new(),
            Message::TargetValue(5).into(),
        );
    }

    #[test]
    #[should_panic(expected = "this message comes only from the config account")]
    fn a_hub_message_from_another_actor_of_the_program_is_refused() {
        let _transition = run(
            actor(STRANGER),
            Some(actor(DEPLOYER)),
            false,
            Vec::new(),
            Message::TargetValue(5).into(),
        );
    }

    #[test]
    #[should_panic(expected = "this message comes only from the config account")]
    fn a_hub_message_at_the_config_is_refused() {
        let _transition = at_config_from(
            Some(actor(DEPLOYER)),
            bytes(&initialized()),
            Message::TargetValue(5).into(),
        );
    }

    #[test]
    #[should_panic(expected = "this message comes only from the config account")]
    fn a_target_value_sent_through_a_seat_is_refused() {
        let _transition = through_seat(
            DEPLOYER,
            None,
            true,
            bytes(&initialized()),
            Message::TargetValue(5).into(),
        );
    }

    /// The admin's own message, through the deployer's seat, handled by `handler`.
    fn admin_message_handled_by(handler: fn(&ReceiveInput, Message) -> Response) -> Response {
        let hop = forwarded(None, &Message::SetAdminValue(1).into());
        let input = input(
            config_actor(),
            Some(actor(DEPLOYER)),
            false,
            bytes(&initialized()),
            &hop,
        );
        lez_admin_authority::guard::<(lez_admin_authority::Admin,), Config, Message>(
            &input,
            &CONFIG_SEED,
            InitGate::Deployer,
            hop,
            handler,
        )
    }

    #[test]
    fn the_admin_s_message_may_call_another_program() {
        let response = admin_message_handled_by(|_input, message| {
            Response::keep_state().send_sync(Actor::new(STRANGER, GOVERNANCE), &message)
        });
        let [sent] = response.sync_messages.as_slice() else {
            panic!("one message to the other program");
        };
        assert_eq!(sent.to, Actor::new(STRANGER, GOVERNANCE));
    }

    #[test]
    #[should_panic(expected = "a handler must not change the admin slot")]
    fn a_handler_that_overwrites_the_slot_is_refused() {
        let _response = admin_message_handled_by(|_input, _message| {
            let config = Config {
                admin: AuthoritySlot::Unset,
                ..initialized()
            };
            Response::set_state(borsh::to_vec(&config).unwrap())
        });
    }
}
