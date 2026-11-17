//! The token sample, written as a plain LEZ program. Its config lives on the config account, a
//! PDA of the program id and `CONFIG_SEED`, and holds the admin's slot, the freeze authority's
//! and the admin's notice. Each owner's balance lives at the owner's own actor under the
//! program. The admin mints: the config credits the owner's actor. While the program is frozen
//! the mint is refused, and the admin's notice, marked exempt, still runs. The attribute puts
//! the guard in front of the handler, and the guard runs the admin's and the freeze
//! authority's operations without it. Only the deployer, the program's own account, sends the
//! admin's `Init`. The first write to the config account needs
//! `PublicAccountEvidence::Pda { program, seed }` in the transaction.

use borsh::BorshDeserialize;
use lee_core::account::Actor;
use lee_core::program::{ReceiveInput, Response};
use lez_freeze_authority::{Admin, Freeze};
use sample_core::{Balance, CONFIG_SEED, Config, Message, NOTICE_MAX};

#[lez_freeze_authority::guard(roles(Admin, Freeze), config = Config, seed = CONFIG_SEED)]
fn handle_message(input: &ReceiveInput, message: Message) -> Response {
    match message {
        Message::Mint { to, amount } => {
            let owner = Actor::new(to, input.receiver.program_account_id);
            Response::keep_state().send_sync(owner, &Message::Credit(amount))
        }
        Message::Credit(amount) => {
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
    use lee_core::{
        account::{AccountId, ActorState},
        program::{SyncMessage, Transition},
    };
    use lez_freeze_authority::{
        AuthoritySlot, FreezeOp, Holder, Op, Slot, Wire, admin_op, freeze_authority_op, freeze_op,
    };
    use sample_core::config_account;

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
        let [SyncMessage { to, message, .. }] =
            <[SyncMessage; 1]>::try_from(at_config.response.sync_messages).unwrap();
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
    #[should_panic(expected = "this message comes only from the config account")]
    fn only_the_config_credits() {
        let _transition = run(
            actor(OWNER),
            Some(actor(STRANGER)),
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
        let [SyncMessage { to, message, .. }] =
            <[SyncMessage; 1]>::try_from(at_config.response.sync_messages).unwrap();
        assert_eq!(to, actor(OWNER));
        let credit: Wire<Message> = borsh::from_slice(&message).unwrap();
        assert_eq!(credit, Message::Credit(5).into());
    }
}
