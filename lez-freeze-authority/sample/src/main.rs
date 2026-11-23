//! The token sample, written as a plain LEZ program. Its config lives on the config account, a
//! PDA of the program id and `CONFIG_SEED`, and holds the admin's slot, the freeze authority's
//! and the admin's notice. Each owner's balance lives at the owner's own actor under the
//! program. The admin mints: the config credits the owner's actor. An owner transfers by
//! sending `Transfer`, signed, straight from the transaction to its own actor, which debits it
//! and credits the recipient's actor. An owner that cannot sign, such as a PDA, is credited but
//! never sends. While the program is frozen the mint and the transfer are refused, the transfer
//! through the requests for clearance its actors send the config, and the admin's notice,
//! marked exempt, still runs. The attribute puts the guard in front of the handler, and the
//! guard runs the admin's and the freeze authority's operations without it. Only the deployer,
//! the program's own account, sends the admin's `Init`. The first write to the config account
//! needs `PublicAccountEvidence::Pda { program, seed }` in the transaction, and every
//! transaction that runs a message away from the config names the config actor. A credit is a
//! sync message, for a mint and a transfer alike, so it reaches a private account only in a
//! transaction that proves for that account.

use borsh::BorshDeserialize;
use freeze_sample_core::{Balance, CONFIG_SEED, Config, Message, NOTICE_MAX};
use lee_core::account::Actor;
use lee_core::program::{ReceiveInput, Response};
use lez_freeze_authority::{Admin, Freeze};

#[lez_freeze_authority::guard(roles(Admin, Freeze), config = Config, seed = CONFIG_SEED)]
fn handle_message(input: &ReceiveInput, message: Message) -> Response {
    match message {
        Message::Mint { to, amount } => {
            let owner = Actor::new(to, input.receiver.program_account_id);
            Response::keep_state().send_sync(owner, &Message::Credit(amount))
        }
        Message::Credit(amount) => {
            assert!(
                input.from_own_program(),
                "a credit comes only from this program's own actors"
            );
            let mut balance: Balance = state(input);
            balance.amount = balance
                .amount
                .checked_add(amount)
                .expect("the balance fits in a u64");
            Response::set_state(borsh::to_vec(&balance).expect("the balance encodes"))
        }
        Message::SetNotice(notice) => {
            assert!(
                notice.len() <= NOTICE_MAX,
                "a notice holds at most {NOTICE_MAX} bytes"
            );
            let mut config: Config = state(input);
            config.notice = notice;
            Response::set_state(borsh::to_vec(&config).expect("the config encodes"))
        }
        Message::Transfer { to, amount } => {
            assert!(input.is_authorized, "the sender must sign the transfer");
            // rc3 authorizes per transaction, so a relaying program would act on the signature.
            assert!(
                input.from.is_none(),
                "a transfer is sent by the transaction itself"
            );
            let mut balance: Balance = state(input);
            balance.amount = balance
                .amount
                .checked_sub(amount)
                .expect("the balance covers the transfer");
            let recipient = Actor::new(to, input.receiver.program_account_id);
            Response::set_state(borsh::to_vec(&balance).expect("the balance encodes"))
                .send_sync(recipient, &Message::Credit(amount))
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
    use std::collections::BTreeMap;

    use freeze_sample_core::config_account;
    use lee_core::{
        account::{AccountId, ActorState},
        program::{SyncMessage, Transition},
    };
    use lez_freeze_authority::lez_authority::LibraryMessage;
    use lez_freeze_authority::{
        AuthoritySlot, FreezeOp, Holder, Op, Slot, Wire, admin_op, freeze_authority_op, freeze_op,
        marker_actor,
    };

    use super::*;

    const PROGRAM: AccountId = AccountId::new([0xC1; 32]);
    const DEPLOYER: AccountId = PROGRAM;
    const OWNER: AccountId = AccountId::new([1; 32]);
    const AUTHORITY: AccountId = AccountId::new([2; 32]);
    const STRANGER: AccountId = AccountId::new([3; 32]);
    const SUCCESSOR: AccountId = AccountId::new([4; 32]);

    fn actor(account: AccountId) -> Actor {
        Actor::new(account, PROGRAM)
    }

    fn config_actor() -> Actor {
        actor(config_account(PROGRAM))
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
        let hop = run(actor(sender), None, true, Vec::new(), message);
        let [SyncMessage { message, .. }] =
            <[SyncMessage; 1]>::try_from(hop.response.sync_messages).unwrap();
        run(
            config_actor(),
            Some(actor(sender)),
            false,
            pre,
            borsh::from_slice(&message).unwrap(),
        )
    }

    fn initialized() -> Vec<u8> {
        let config = Config {
            admin: AuthoritySlot::held(Holder {
                account: DEPLOYER,
                via: None,
            }),
            ..Config::default()
        };
        borsh::to_vec(&config).unwrap()
    }

    #[test]
    fn the_deployer_initializes_the_config() {
        let init = through_seat(DEPLOYER, Vec::new(), admin_op(Op::Init));
        let written: Config =
            borsh::from_slice(init.response.post_state.as_ref().unwrap()).unwrap();
        assert_eq!(borsh::to_vec(&written).unwrap(), initialized());
    }

    #[test]
    fn the_admin_mints_and_the_config_credits_the_owner_s_actor() {
        let mint = Message::Mint {
            to: OWNER,
            amount: 5,
        };
        let at_config = through_seat(DEPLOYER, initialized(), mint.into());
        assert!(at_config.response.post_state.is_none());
        let [check, SyncMessage { to, message, .. }] =
            <[SyncMessage; 2]>::try_from(at_config.response.sync_messages).unwrap();
        assert_eq!(check.to, marker_actor(config_actor(), DEPLOYER));
        assert_eq!(to, actor(OWNER));
        let credit: Wire<Message> = borsh::from_slice(&message).unwrap();
        let credited = run(
            actor(OWNER),
            Some(config_actor()),
            false,
            Vec::new(),
            credit,
        );
        let balance: Balance =
            borsh::from_slice(credited.response.post_state.as_ref().unwrap()).unwrap();
        assert_eq!(balance, Balance { amount: 5 });
        let again = run(
            actor(OWNER),
            Some(config_actor()),
            false,
            borsh::to_vec(&balance).unwrap(),
            Message::Credit(3).into(),
        );
        let balance: Balance =
            borsh::from_slice(again.response.post_state.as_ref().unwrap()).unwrap();
        assert_eq!(balance, Balance { amount: 8 });
    }

    #[test]
    fn the_admin_offers_the_freeze_authority_role() {
        let offered = through_seat(
            DEPLOYER,
            initialized(),
            freeze_authority_op(Op::Nominate { next: OWNER }),
        );
        let mut expected: Config = borsh::from_slice(&initialized()).unwrap();
        *expected.freeze.authority_slot_mut() = AuthoritySlot::Offered { nominee: OWNER };
        let written: Config =
            borsh::from_slice(offered.response.post_state.as_ref().unwrap()).unwrap();
        assert_eq!(written, expected);
    }

    #[test]
    #[should_panic(
        expected = "lez-authority/freeze E137: the freeze authority is appointed by nomination, never installed"
    )]
    fn the_freeze_role_has_no_init() {
        let _transition = through_seat(DEPLOYER, initialized(), freeze_authority_op(Op::Init));
    }

    #[test]
    #[should_panic(expected = "the balance fits in a u64")]
    fn a_credit_past_the_largest_balance_is_refused() {
        let full = borsh::to_vec(&Balance { amount: u64::MAX }).unwrap();
        let _transition = run(
            actor(OWNER),
            Some(config_actor()),
            false,
            full,
            Message::Credit(1).into(),
        );
    }

    #[test]
    #[should_panic(expected = "lez-authority/admin E103: the signing account is not the admin")]
    fn only_the_admin_mints() {
        let mint = Message::Mint {
            to: STRANGER,
            amount: 5,
        };
        let _transition = through_seat(STRANGER, initialized(), mint.into());
    }

    #[test]
    #[should_panic(expected = "a credit comes only from this program's own actors")]
    fn a_client_s_credit_is_refused() {
        let _transition = run(
            actor(OWNER),
            None,
            true,
            Vec::new(),
            Message::Credit(5).into(),
        );
    }

    #[test]
    #[should_panic(expected = "a credit comes only from this program's own actors")]
    fn a_signed_credit_from_another_program_is_refused() {
        let _transition = run(
            actor(OWNER),
            Some(Actor::new(STRANGER, STRANGER)),
            true,
            Vec::new(),
            Message::Credit(5).into(),
        );
    }

    #[test]
    #[should_panic(expected = "a credit comes only from this program's own actors")]
    fn a_credit_from_another_program_is_refused() {
        let _transition = run(
            actor(OWNER),
            Some(Actor::new(STRANGER, STRANGER)),
            false,
            Vec::new(),
            Message::Credit(5).into(),
        );
    }

    fn written(transition: &Transition) -> Config {
        borsh::from_slice(transition.response.post_state.as_ref().unwrap()).unwrap()
    }

    fn holder(account: AccountId) -> AuthoritySlot {
        AuthoritySlot::held(Holder { account, via: None })
    }

    /// The initialized config with `freeze` as the freeze authority.
    fn appointed(freeze: AccountId) -> Config {
        let mut config: Config = borsh::from_slice(&initialized()).unwrap();
        *config.freeze.authority_slot_mut() = holder(freeze);
        config
    }

    /// `message` from `sender`'s seat to the config as `pre` holds it. The config as written.
    fn send(sender: AccountId, pre: &Config, message: Wire<Message>) -> Config {
        written(&through_seat(sender, borsh::to_vec(pre).unwrap(), message))
    }

    /// The config once `AUTHORITY` froze the program.
    fn frozen() -> Config {
        send(
            AUTHORITY,
            &appointed(AUTHORITY),
            freeze_op(FreezeOp::Freeze),
        )
    }

    fn notice(text: &str) -> Wire<Message> {
        Message::SetNotice(text.to_owned()).into()
    }

    fn mint(amount: u64) -> Wire<Message> {
        Message::Mint { to: OWNER, amount }.into()
    }

    #[test]
    fn the_admin_s_notice_replaces_the_last_one() {
        let first = send(DEPLOYER, &appointed(AUTHORITY), notice("hello"));
        assert_eq!(
            first,
            Config {
                notice: "hello".to_owned(),
                ..appointed(AUTHORITY)
            }
        );
        assert_eq!(
            send(DEPLOYER, &first, notice("bye")),
            Config {
                notice: "bye".to_owned(),
                ..appointed(AUTHORITY)
            }
        );
    }

    #[test]
    fn a_notice_of_the_longest_length_is_posted() {
        let longest = "x".repeat(NOTICE_MAX);
        assert_eq!(
            send(DEPLOYER, &appointed(AUTHORITY), notice(&longest)).notice,
            longest
        );
    }

    // The cap counts bytes, since every message at the config decodes the notice.
    #[test]
    #[should_panic(expected = "a notice holds at most 64 bytes")]
    fn a_notice_of_the_longest_length_in_characters_but_longer_in_bytes_is_refused() {
        let wide = "x".repeat(NOTICE_MAX - 1) + "é";
        send(DEPLOYER, &appointed(AUTHORITY), notice(&wide));
    }

    #[test]
    #[should_panic(expected = "a notice holds at most 64 bytes")]
    fn a_longer_notice_is_refused() {
        send(
            DEPLOYER,
            &appointed(AUTHORITY),
            notice(&"x".repeat(NOTICE_MAX + 1)),
        );
    }

    #[test]
    #[should_panic(expected = "lez-freeze-authority/freeze E217: the program is frozen")]
    fn a_freeze_refuses_the_mint() {
        send(DEPLOYER, &frozen(), mint(5));
    }

    #[test]
    fn the_admin_s_notice_runs_through_a_freeze() {
        let posted = send(DEPLOYER, &frozen(), notice("frozen for review"));
        assert!(posted.freeze.is_frozen());
        assert_eq!(
            posted,
            Config {
                notice: "frozen for review".to_owned(),
                ..frozen()
            }
        );
    }

    #[test]
    #[should_panic(expected = "lez-authority/admin E103: the signing account is not the admin")]
    fn a_freeze_leaves_the_notice_to_the_admin() {
        send(STRANGER, &frozen(), notice("frozen for review"));
    }

    #[test]
    fn the_admin_replaces_the_freeze_authority_of_a_frozen_program() {
        let nominated = send(
            DEPLOYER,
            &frozen(),
            freeze_authority_op(Op::Nominate { next: SUCCESSOR }),
        );
        let replaced = send(
            SUCCESSOR,
            &nominated,
            freeze_authority_op(Op::Accept { pda_seed: None }),
        );
        assert!(replaced.freeze.is_frozen());
        assert_eq!(replaced.freeze.authority_slot(), &holder(SUCCESSOR));
        assert_eq!(
            send(SUCCESSOR, &replaced, freeze_op(FreezeOp::Release)),
            appointed(SUCCESSOR)
        );
    }

    #[test]
    #[should_panic(
        expected = "lez-authority/freeze E103: the signing account is not the freeze authority"
    )]
    fn a_replaced_freeze_authority_cannot_release() {
        let nominated = send(
            DEPLOYER,
            &frozen(),
            freeze_authority_op(Op::Nominate { next: SUCCESSOR }),
        );
        let replaced = send(
            SUCCESSOR,
            &nominated,
            freeze_authority_op(Op::Accept { pda_seed: None }),
        );
        send(AUTHORITY, &replaced, freeze_op(FreezeOp::Release));
    }

    #[test]
    fn after_a_release_the_admin_mints_again() {
        let released = send(AUTHORITY, &frozen(), freeze_op(FreezeOp::Release));
        assert_eq!(released, appointed(AUTHORITY));
        let at_config = through_seat(DEPLOYER, borsh::to_vec(&released).unwrap(), mint(5));
        let [check, SyncMessage { to, message, .. }] =
            <[SyncMessage; 2]>::try_from(at_config.response.sync_messages).unwrap();
        assert_eq!(check.to, marker_actor(config_actor(), DEPLOYER));
        assert_eq!(to, actor(OWNER));
        let credit: Wire<Message> = borsh::from_slice(&message).unwrap();
        assert_eq!(credit, Message::Credit(5).into());
    }

    /// Every actor's state in one transaction, as the runtime keeps it.
    type States = BTreeMap<Actor, Vec<u8>>;

    /// Runs `message` at `to` as the runtime does: the handler over the actor's state, the state
    /// it writes, then each message it sends, depth first. `signers` signed the transaction. The
    /// requests for clearance it answered, as the sending actor's account and the bit it carried.
    /// It keeps what a refused chain wrote before the refusal, which the runtime reverts, so only
    /// the live run shows that a refusal writes nothing.
    fn execute(
        states: &mut States,
        to: Actor,
        from: Option<Actor>,
        signers: &[AccountId],
        message: Vec<u8>,
    ) -> Vec<(AccountId, bool)> {
        let wire: Wire<Message> = borsh::from_slice(&message).unwrap();
        let input = ReceiveInput {
            receiver: to,
            from,
            is_authorized: signers.contains(&to.account_id),
            pre_state: ActorState::from(states.get(&to).cloned().unwrap_or_default()),
            message,
        };
        let transition = handle_message(&input, wire).into_transition(input);
        if let Some(post) = &transition.response.post_state {
            states.insert(to, post.to_vec());
        }
        let mut cleared = Vec::new();
        for sent in transition.response.sync_messages {
            if let Wire::Library(LibraryMessage::Clearance(request)) =
                borsh::from_slice::<Wire<Message>>(&sent.message).unwrap()
            {
                cleared.push((to.account_id, request.authorized));
            }
            cleared.extend(execute(states, sent.to, Some(to), signers, sent.message));
        }
        cleared
    }

    /// The config as `config` holds it, and each owner's balance.
    fn states(config: &Config, balances: &[(AccountId, u64)]) -> States {
        let mut states = States::from([(config_actor(), borsh::to_vec(config).unwrap())]);
        for &(owner, amount) in balances {
            states.insert(actor(owner), borsh::to_vec(&Balance { amount }).unwrap());
        }
        states
    }

    fn balance(states: &States, owner: AccountId) -> u64 {
        states.get(&actor(owner)).map_or(0, |state| {
            borsh::from_slice::<Balance>(state).unwrap().amount
        })
    }

    /// `from`'s transfer of `amount` to `to`, sent to `from`'s actor and signed by `signers`. The
    /// requests for clearance it answered.
    fn transfer(
        states: &mut States,
        from: AccountId,
        to: AccountId,
        amount: u64,
        signers: &[AccountId],
    ) -> Vec<(AccountId, bool)> {
        let message = borsh::to_vec(&Wire::Program(Message::Transfer { to, amount })).unwrap();
        execute(states, actor(from), None, signers, message)
    }

    #[test]
    fn a_transfer_moves_the_amount_from_the_sender_s_actor_to_the_recipient_s() {
        let mut states = states(&appointed(AUTHORITY), &[(OWNER, 5)]);
        let cleared = transfer(&mut states, OWNER, SUCCESSOR, 3, &[OWNER]);
        assert_eq!(cleared, [(SUCCESSOR, false), (OWNER, true)]);
        assert_eq!(balance(&states, OWNER), 2);
        assert_eq!(balance(&states, SUCCESSOR), 3);
        assert_eq!(
            states[&config_actor()],
            borsh::to_vec(&appointed(AUTHORITY)).unwrap()
        );
    }

    #[test]
    fn a_transfer_to_oneself_keeps_the_balance() {
        let mut states = states(&appointed(AUTHORITY), &[(OWNER, 5)]);
        transfer(&mut states, OWNER, OWNER, 2, &[OWNER]);
        assert_eq!(balance(&states, OWNER), 5);
    }

    #[test]
    #[should_panic(expected = "lez-freeze-authority/freeze E217: the program is frozen")]
    fn a_freeze_refuses_a_transfer() {
        let mut states = states(&frozen(), &[(OWNER, 5)]);
        transfer(&mut states, OWNER, SUCCESSOR, 3, &[OWNER]);
    }

    #[test]
    #[should_panic(expected = "lez-freeze-authority/freeze E217: the program is frozen")]
    fn a_freeze_refuses_a_credit_too() {
        let mut states = states(&frozen(), &[]);
        let credit = borsh::to_vec(&Wire::Program(Message::Credit(1))).unwrap();
        execute(&mut states, actor(OWNER), Some(config_actor()), &[], credit);
    }

    #[test]
    #[should_panic(expected = "the sender must sign the transfer")]
    fn a_transfer_needs_the_sender_s_signature() {
        let mut states = states(&appointed(AUTHORITY), &[(OWNER, 5)]);
        transfer(&mut states, OWNER, SUCCESSOR, 3, &[STRANGER]);
    }

    #[test]
    #[should_panic(expected = "the balance covers the transfer")]
    fn a_transfer_beyond_the_balance_is_refused() {
        let mut states = states(&appointed(AUTHORITY), &[(OWNER, 5)]);
        transfer(&mut states, OWNER, SUCCESSOR, 6, &[OWNER]);
    }

    #[test]
    fn a_transfer_after_a_release_lands() {
        let released = send(AUTHORITY, &frozen(), freeze_op(FreezeOp::Release));
        let mut states = states(&released, &[(OWNER, 5)]);
        transfer(&mut states, OWNER, SUCCESSOR, 5, &[OWNER]);
        assert_eq!(balance(&states, OWNER), 0);
        assert_eq!(balance(&states, SUCCESSOR), 5);
    }

    #[test]
    fn a_co_signed_transfer_asks_with_both_accounts_authorized() {
        let mut states = states(&appointed(AUTHORITY), &[(OWNER, 5)]);
        let cleared = transfer(&mut states, OWNER, SUCCESSOR, 3, &[OWNER, SUCCESSOR]);
        assert_eq!(cleared, [(SUCCESSOR, true), (OWNER, true)]);
    }

    #[test]
    fn an_unsigned_transfer_is_refused_whoever_relays_it() {
        for from in [
            Some(actor(STRANGER)),
            Some(Actor::new(STRANGER, STRANGER)),
            Some(config_actor()),
            Some(Actor::new(OWNER, STRANGER)),
            Some(actor(OWNER)),
        ] {
            let mut states = states(&appointed(AUTHORITY), &[(OWNER, 5)]);
            let message = borsh::to_vec(&Wire::Program(Message::Transfer {
                to: SUCCESSOR,
                amount: 3,
            }))
            .unwrap();
            let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                execute(&mut states, actor(OWNER), from, &[STRANGER], message)
            }))
            .expect_err("an unsigned transfer is refused");
            let text = refused
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| refused.downcast_ref::<String>().map(String::as_str))
                .unwrap_or_default();
            assert!(
                text.contains("the sender must sign the transfer"),
                "{from:?}: {text}"
            );
        }
    }

    #[test]
    fn a_signed_transfer_is_refused_when_relayed() {
        for from in [
            Some(actor(STRANGER)),
            Some(Actor::new(STRANGER, STRANGER)),
            Some(config_actor()),
            Some(Actor::new(OWNER, STRANGER)),
            Some(actor(OWNER)),
        ] {
            let mut states = states(&appointed(AUTHORITY), &[(OWNER, 5)]);
            let message = borsh::to_vec(&Wire::Program(Message::Transfer {
                to: SUCCESSOR,
                amount: 3,
            }))
            .unwrap();
            let refused = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                execute(&mut states, actor(OWNER), from, &[OWNER], message)
            }))
            .expect_err("a relayed transfer is refused");
            let text = refused
                .downcast_ref::<&str>()
                .copied()
                .or_else(|| refused.downcast_ref::<String>().map(String::as_str))
                .unwrap_or_default();
            assert!(
                text.contains("a transfer is sent by the transaction itself"),
                "{from:?}: {text}"
            );
        }
    }
}
