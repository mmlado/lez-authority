use lee_core::account::{Actor, ActorState};
use lee_core::program::{PdaSeed, ReceiveInput, Response};
use lez_authority::{Clearance, LibraryMessage, try_guard};

use super::*;

const PROGRAM: AccountId = AccountId::new([0xF0; 32]);
const ADMIN: AccountId = PROGRAM;
const AUTHORITY: AccountId = AccountId::new([1; 32]);
const SUCCESSOR: AccountId = AccountId::new([2; 32]);
const STRANGER: AccountId = AccountId::new([3; 32]);
const ELSEWHERE: AccountId = AccountId::new([0xE0; 32]);
const MULTISIG: AccountId = AccountId::new([0xB0; 32]);
const GOVERNANCE: AccountId = AccountId::new([0xC0; 32]);
const SEED: PdaSeed = seed(b"lez-freeze-authority/tests/v1");

#[derive(Clone, Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, HasSlot)]
struct Config {
    #[slot(Admin)]
    admin: AuthoritySlot,
    #[slot(Freeze)]
    freeze: FreezeSlot,
    value: u64,
}

#[derive(BorshSerialize, BorshDeserialize, Gated)]
enum Message {
    /// Changes one part of the freeze slot, or nothing for any other number.
    Touch(u8),
    Set(u64),
    #[only(Admin)]
    AdminSet(u64),
    #[only(Freeze)]
    AuthoritySet(u64),
    #[exempt(Freeze)]
    ExemptSet(u64),
    #[only(Admin)]
    #[exempt(Freeze)]
    AdminExemptSet(u64),
    #[exempt(Freeze)]
    ExemptTouch(u8),
    /// Adds to a counter at the actor it reaches, away from the config.
    Count(u8),
    #[exempt(Freeze)]
    ExemptCount(u8),
    #[from_config]
    Credit(u8),
}

fn handle(input: &ReceiveInput, message: Message) -> Response {
    if let Message::Count(more) | Message::ExemptCount(more) | Message::Credit(more) = message {
        let count = input.pre_state.first().copied().unwrap_or(0);
        return Response::set_state(vec![count + more]);
    }
    let mut config: Config = borsh::from_slice(&input.pre_state).unwrap();
    match message {
        Message::Touch(part) | Message::ExemptTouch(part) => match part {
            0 => *config.freeze.authority_slot_mut() = holder(STRANGER),
            1 => config.freeze.frozen = !config.freeze.frozen,
            2 => config.freeze.frozen_accounts.push(SUCCESSOR),
            3 => config.freeze.frozen_accounts.reverse(),
            _ => {}
        },
        Message::Set(value)
        | Message::AdminSet(value)
        | Message::AuthoritySet(value)
        | Message::ExemptSet(value)
        | Message::AdminExemptSet(value) => config.value = value,
        Message::Count(_) | Message::ExemptCount(_) | Message::Credit(_) => unreachable!(),
    }
    Response::set_state(borsh::to_vec(&config).unwrap())
}

fn config_actor() -> Actor {
    Actor::new(config_account(PROGRAM, &SEED), PROGRAM)
}

fn at(
    receiver: Actor,
    from: Option<Actor>,
    authorized: bool,
    pre: Option<&Config>,
    message: Wire<Message>,
) -> Result<Response, Refusal> {
    let input = ReceiveInput {
        receiver,
        from,
        is_authorized: authorized,
        pre_state: ActorState::from(pre.map(|c| borsh::to_vec(c).unwrap()).unwrap_or_default()),
        message: borsh::to_vec(&message).unwrap(),
    };
    try_guard::<(Admin, Freeze), Config, Message>(
        &input,
        &SEED,
        InitGate::Deployer,
        message,
        handle,
    )
}

/// From `sender`'s seat, reached at top level or through `via`, to the config. The config as
/// written back.
fn send_via(
    sender: AccountId,
    via: Option<AccountId>,
    pre: Option<&Config>,
    message: Wire<Message>,
) -> Result<Config, Refusal> {
    let seat = Actor::new(sender, PROGRAM);
    let from = via.map(|program| Actor::new(program, program));
    let hop = at(seat, from, true, None, message)?;
    let hop: Wire<Message> = borsh::from_slice(&hop.sync_messages[0].message).unwrap();
    let response = at(config_actor(), Some(seat), false, pre, hop)?;
    Ok(borsh::from_slice(response.post_state.as_ref().unwrap()).unwrap())
}

fn send(sender: AccountId, pre: &Config, message: Wire<Message>) -> Result<Config, Refusal> {
    send_via(sender, None, Some(pre), message)
}

fn holder(account: AccountId) -> AuthoritySlot {
    AuthoritySlot::held(Holder { account, via: None })
}

/// A config the admin initialized, with `freeze` as the freeze slot's holder part.
fn config(freeze: AuthoritySlot) -> Config {
    Config {
        admin: holder(ADMIN),
        freeze: FreezeSlot {
            authority_slot: freeze,
            ..FreezeSlot::default()
        },
        value: 0,
    }
}

fn nominate(next: AccountId) -> Wire<Message> {
    freeze_authority_op(Op::Nominate { next })
}

fn accept() -> Wire<Message> {
    freeze_authority_op(Op::Accept { pda_seed: None })
}

fn renounce() -> Wire<Message> {
    freeze_authority_op(Op::Renounce)
}

fn freeze() -> Wire<Message> {
    freeze_op(FreezeOp::Freeze)
}

fn release() -> Wire<Message> {
    freeze_op(FreezeOp::Release)
}

fn freeze_account(account: AccountId) -> Wire<Message> {
    freeze_op(FreezeOp::FreezeAccount { account })
}

fn release_account(account: AccountId) -> Wire<Message> {
    freeze_op(FreezeOp::ReleaseAccount { account })
}

/// `config` with `accounts` as the frozen accounts.
fn with_frozen(mut config: Config, accounts: &[AccountId]) -> Config {
    config.freeze.frozen_accounts = accounts.to_vec();
    config
}

/// An id set only in its first and last bytes, so ids can differ past their first byte.
fn id(first: u8, last: u8) -> AccountId {
    let mut bytes = [0; 32];
    bytes[0] = first;
    bytes[31] = last;
    AccountId::new(bytes)
}

/// `config` with the program frozen.
fn frozen(mut config: Config) -> Config {
    config.freeze.frozen = true;
    config
}

/// `config` with the value at `value`.
fn valued(mut config: Config, value: u64) -> Config {
    config.value = value;
    config
}

/// `message` sent straight to the config. The config as written back.
fn straight(pre: &Config, message: Message) -> Result<Config, Refusal> {
    let response = at(config_actor(), None, false, Some(pre), message.into())?;
    Ok(borsh::from_slice(response.post_state.as_ref().unwrap()).unwrap())
}

fn e217() -> Refusal {
    Refusal::of::<Freeze>(FreezeError::Frozen.into())
}

fn e208() -> Refusal {
    Refusal::of::<Freeze>(FreezeError::NotAdminOrFreezeAuthority.into())
}

#[test]
fn the_freeze_codes_are_stable() {
    let expected: [(FreezeError, u16); 8] = [
        (FreezeError::NotAdminOrFreezeAuthority, 208),
        (FreezeError::AlreadyFrozen, 211),
        (FreezeError::NotFrozen, 212),
        (FreezeError::AccountAlreadyFrozen, 213),
        (FreezeError::AccountNotFrozen, 214),
        (FreezeError::Frozen, 217),
        (FreezeError::AccountFrozen, 218),
        (FreezeError::ZeroAccount, 225),
    ];
    for (error, code) in expected {
        assert_eq!(error.code(), code, "{error:?}");
    }
    assert_eq!(FreezeError::ALL, expected.map(|(error, _)| error));
    assert_eq!(
        e208().text(),
        "lez-freeze-authority/freeze E208: the signing account is neither the freeze authority nor the admin"
    );
    assert_eq!(
        Refusal::of::<Freeze>(FreezeError::AlreadyFrozen.into()).text(),
        "lez-freeze-authority/freeze E211: the program is already frozen"
    );
    assert_eq!(
        Refusal::of::<Freeze>(FreezeError::NotFrozen.into()).text(),
        "lez-freeze-authority/freeze E212: the program is not frozen"
    );
    assert_eq!(
        Refusal::of::<Freeze>(FreezeError::AccountAlreadyFrozen.into()).text(),
        "lez-freeze-authority/freeze E213: the account is already frozen"
    );
    assert_eq!(
        Refusal::of::<Freeze>(FreezeError::AccountNotFrozen.into()).text(),
        "lez-freeze-authority/freeze E214: the account is not frozen"
    );
    assert_eq!(
        e217().text(),
        "lez-freeze-authority/freeze E217: the program is frozen"
    );
    assert_eq!(
        Refusal::of::<Freeze>(FreezeError::AccountFrozen.into()).text(),
        "lez-freeze-authority/freeze E218: the signing account is frozen"
    );
    assert_eq!(
        Refusal::of::<Freeze>(FreezeError::ZeroAccount.into()).text(),
        "lez-freeze-authority/freeze E225: the all-zero id names no account"
    );
}

#[test]
fn the_freeze_slot_encodes_its_holder_part_the_flag_and_the_frozen_accounts() {
    assert_eq!(
        borsh::to_vec(&FreezeSlot::default()).unwrap(),
        [0, 0, 0, 0, 0, 0]
    );
    let slot = FreezeSlot {
        authority_slot: AuthoritySlot::Unset,
        frozen: true,
        frozen_accounts: vec![STRANGER],
    };
    let id = borsh::to_vec(&STRANGER).unwrap();
    assert_eq!(
        borsh::to_vec(&slot).unwrap(),
        [&[0, 1, 1, 0, 0, 0][..], &id].concat()
    );
}

#[test]
fn the_admin_s_init_creates_the_freeze_slot_unset_with_nothing_frozen() {
    let created = send_via(ADMIN, None, None, admin_op(Op::Init)).unwrap();
    assert_eq!(created, config(AuthoritySlot::Unset));
    assert_eq!(created.freeze, FreezeSlot::default());
}

#[test]
fn the_freeze_role_has_no_init() {
    let refused = send(
        ADMIN,
        &config(AuthoritySlot::Unset),
        freeze_authority_op(Op::Init),
    );
    assert_eq!(
        refused,
        Err(Refusal::of::<Freeze>(AuthorityError::ByNominationOnly))
    );
    assert_eq!(
        refused.unwrap_err().text(),
        "lez-authority/freeze E137: the freeze authority is appointed by nomination, never installed"
    );
}

#[test]
fn the_admin_appoints_the_first_freeze_authority() {
    let offered = send(ADMIN, &config(AuthoritySlot::Unset), nominate(AUTHORITY)).unwrap();
    assert_eq!(
        offered,
        config(AuthoritySlot::Offered { nominee: AUTHORITY })
    );
    let appointed = send(AUTHORITY, &offered, accept()).unwrap();
    assert_eq!(appointed, config(holder(AUTHORITY)));
}

#[test]
fn the_freeze_authority_or_the_admin_names_a_successor() {
    for nominator in [AUTHORITY, ADMIN] {
        let nominated = send(nominator, &config(holder(AUTHORITY)), nominate(SUCCESSOR)).unwrap();
        assert_eq!(nominated.freeze.authority_slot().nominee(), Some(SUCCESSOR));
        let handed_on = send(SUCCESSOR, &nominated, accept()).unwrap();
        assert_eq!(handed_on, config(holder(SUCCESSOR)));
    }
}

#[test]
fn anyone_else_is_refused_with_e208() {
    for pre in [
        config(AuthoritySlot::Unset),
        config(AuthoritySlot::Offered { nominee: AUTHORITY }),
        config(holder(AUTHORITY)),
    ] {
        assert_eq!(send(STRANGER, &pre, nominate(STRANGER)), Err(e208()));
        assert_eq!(send(STRANGER, &pre, renounce()), Err(e208()));
    }
}

#[test]
fn the_nominee_cannot_act_for_the_role_before_it_accepts() {
    let offered = config(AuthoritySlot::Offered { nominee: AUTHORITY });
    assert_eq!(send(AUTHORITY, &offered, nominate(SUCCESSOR)), Err(e208()));
    assert_eq!(send(AUTHORITY, &offered, renounce()), Err(e208()));
}

#[test]
fn a_revoke_leaves_the_slot_unset_for_the_admin_to_fill_again() {
    let revoked = send(ADMIN, &config(holder(AUTHORITY)), renounce()).unwrap();
    assert_eq!(revoked, config(AuthoritySlot::Unset));
    let offered = send(ADMIN, &revoked, nominate(SUCCESSOR)).unwrap();
    let appointed = send(SUCCESSOR, &offered, accept()).unwrap();
    assert_eq!(appointed, config(holder(SUCCESSOR)));
}

#[test]
fn the_freeze_authority_steps_down_and_the_admin_withdraws_an_offer() {
    let stepped_down = send(AUTHORITY, &config(holder(AUTHORITY)), renounce()).unwrap();
    assert_eq!(stepped_down, config(AuthoritySlot::Unset));
    let offered = config(AuthoritySlot::Offered { nominee: AUTHORITY });
    assert_eq!(
        send(ADMIN, &offered, renounce()).unwrap(),
        config(AuthoritySlot::Unset)
    );
}

#[test]
fn a_renounce_leaves_a_standing_freeze_and_the_frozen_accounts() {
    let mut frozen = config(holder(AUTHORITY));
    frozen.freeze.frozen = true;
    frozen.freeze.frozen_accounts = vec![STRANGER];
    let revoked = send(ADMIN, &frozen, renounce()).unwrap();
    assert_eq!(revoked.freeze.authority_slot(), &AuthoritySlot::Unset);
    assert!(revoked.freeze.frozen);
    assert_eq!(revoked.freeze.frozen_accounts, vec![STRANGER]);
}

#[test]
fn once_the_admin_is_renounced_the_freeze_authority_keeps_working_and_an_emptied_slot_stays_empty()
{
    let without_admin = |freeze| Config {
        admin: AuthoritySlot::Renounced,
        ..config(freeze)
    };
    let nominated = send(
        AUTHORITY,
        &without_admin(holder(AUTHORITY)),
        nominate(SUCCESSOR),
    )
    .unwrap();
    let handed_on = send(SUCCESSOR, &nominated, accept()).unwrap();
    assert_eq!(handed_on, without_admin(holder(SUCCESSOR)));
    let emptied = send(SUCCESSOR, &handed_on, renounce()).unwrap();
    assert_eq!(emptied, without_admin(AuthoritySlot::Unset));
    for anyone in [ADMIN, SUCCESSOR, STRANGER] {
        assert_eq!(send(anyone, &emptied, nominate(anyone)), Err(e208()));
    }
}

#[test]
fn a_holder_reached_through_another_program_is_told_so() {
    for message in [nominate(SUCCESSOR), renounce()] {
        let refused = send_via(
            AUTHORITY,
            Some(ELSEWHERE),
            Some(&config(holder(AUTHORITY))),
            message,
        );
        assert_eq!(
            refused,
            Err(Refusal::of::<Freeze>(AuthorityError::ViaNotAllowed))
        );
    }
}

#[test]
fn the_admin_reached_through_another_program_is_told_so() {
    for pre in [
        config(AuthoritySlot::Unset),
        config(AuthoritySlot::Offered { nominee: AUTHORITY }),
        config(holder(AUTHORITY)),
    ] {
        for message in [nominate(SUCCESSOR), renounce()] {
            assert_eq!(
                send_via(ADMIN, Some(ELSEWHERE), Some(&pre), message),
                Err(Refusal::of::<Admin>(AuthorityError::ViaNotAllowed))
            );
        }
    }
}

#[test]
fn an_admin_held_through_a_program_acts_only_through_it() {
    let vault = AccountId::for_public_pda(&MULTISIG, &PdaSeed::new([7; 32]));
    let with_vault = |freeze| Config {
        admin: AuthoritySlot::held(Holder {
            account: vault,
            via: Some(MULTISIG),
        }),
        ..config(freeze)
    };
    let offered = send_via(
        vault,
        Some(MULTISIG),
        Some(&with_vault(AuthoritySlot::Unset)),
        nominate(AUTHORITY),
    )
    .unwrap();
    assert_eq!(
        offered,
        with_vault(AuthoritySlot::Offered { nominee: AUTHORITY })
    );
    let held = with_vault(holder(AUTHORITY));
    let revoked = send_via(vault, Some(MULTISIG), Some(&held), renounce()).unwrap();
    assert_eq!(revoked, with_vault(AuthoritySlot::Unset));
    for via in [None, Some(ELSEWHERE)] {
        assert_eq!(
            send_via(vault, via, Some(&held), renounce()),
            Err(Refusal::of::<Admin>(AuthorityError::ViaNotAllowed))
        );
    }
}

#[test]
fn a_handler_cannot_touch_any_part_of_the_freeze_slot() {
    let mut pre = config(holder(AUTHORITY));
    pre.freeze.frozen_accounts = vec![AUTHORITY, STRANGER];
    let at_config = |part| {
        at(
            config_actor(),
            None,
            false,
            Some(&pre),
            Message::Touch(part).into(),
        )
        .err()
    };
    assert_eq!(at_config(4), None);
    for part in 0..4 {
        assert_eq!(
            at_config(part),
            Some(Refusal::of::<Freeze>(AuthorityError::SlotChanged)),
            "part {part}"
        );
    }
}

#[test]
fn a_revoke_withdraws_a_pending_hand_over() {
    let pending = send(AUTHORITY, &config(holder(AUTHORITY)), nominate(SUCCESSOR)).unwrap();
    let revoked = send(ADMIN, &pending, renounce()).unwrap();
    assert_eq!(revoked, config(AuthoritySlot::Unset));
    assert_eq!(
        send(SUCCESSOR, &revoked, accept()),
        Err(Refusal::of::<Freeze>(AuthorityError::NoNomination))
    );
}

#[test]
fn a_pda_nominee_proves_itself_with_its_seed_and_then_acts_through_its_program() {
    let seed = PdaSeed::new([9; 32]);
    let pda = AccountId::for_public_pda(&GOVERNANCE, &seed);
    let offered = send(ADMIN, &config(AuthoritySlot::Unset), nominate(pda)).unwrap();
    let accept_with =
        |pda_seed: Option<PdaSeed>| freeze_authority_op::<Message>(Op::Accept { pda_seed });
    for pda_seed in [None, Some(PdaSeed::new([8; 32]))] {
        assert_eq!(
            send_via(pda, Some(GOVERNANCE), Some(&offered), accept_with(pda_seed)),
            Err(Refusal::of::<Freeze>(AuthorityError::SeedProofFailed))
        );
    }
    let appointed = send_via(
        pda,
        Some(GOVERNANCE),
        Some(&offered),
        accept_with(Some(seed)),
    )
    .unwrap();
    let governed = Holder {
        account: pda,
        via: Some(GOVERNANCE),
    };
    assert_eq!(appointed, config(AuthoritySlot::held(governed)));
    let nominated = send_via(pda, Some(GOVERNANCE), Some(&appointed), nominate(SUCCESSOR)).unwrap();
    assert_eq!(nominated.freeze.authority_slot().nominee(), Some(SUCCESSOR));
    for via in [None, Some(ELSEWHERE)] {
        assert_eq!(
            send_via(pda, via, Some(&appointed), renounce()),
            Err(Refusal::of::<Freeze>(AuthorityError::ViaNotAllowed))
        );
    }
}

#[test]
fn an_unsigned_sender_is_refused_at_its_seat_in_the_freeze_s_name() {
    for message in [nominate(SUCCESSOR), freeze()] {
        let refused = at(Actor::new(AUTHORITY, PROGRAM), None, false, None, message).err();
        assert_eq!(
            refused,
            Some(Refusal::of::<Freeze>(AuthorityError::MissingSignature))
        );
        assert_eq!(
            refused.unwrap().text(),
            "lez-authority/freeze E104: the sender must authorize the change"
        );
    }
}

#[test]
fn only_the_nominee_accepts_an_offer() {
    let offered = config(AuthoritySlot::Offered { nominee: AUTHORITY });
    for anyone in [STRANGER, ADMIN] {
        assert_eq!(
            send(anyone, &offered, accept()),
            Err(Refusal::of::<Freeze>(AuthorityError::NotNominee))
        );
    }
}

#[test]
fn an_offer_standing_when_the_admin_renounces_can_still_be_accepted_and_nobody_can_withdraw_it() {
    let without_admin = |freeze| Config {
        admin: AuthoritySlot::Renounced,
        ..config(freeze)
    };
    let offered = without_admin(AuthoritySlot::Offered { nominee: AUTHORITY });
    for anyone in [ADMIN, AUTHORITY, STRANGER] {
        assert_eq!(send(anyone, &offered, renounce()), Err(e208()));
        assert_eq!(
            send(anyone, &offered, nominate(AccountId::default())),
            Err(e208())
        );
    }
    assert_eq!(
        send(AUTHORITY, &offered, accept()).unwrap(),
        without_admin(holder(AUTHORITY))
    );
}

#[test]
fn the_zero_id_cancels_and_the_admin_s_nomination_replaces() {
    let cancel = || nominate(AccountId::default());
    let offered = config(AuthoritySlot::Offered { nominee: AUTHORITY });
    assert_eq!(
        send(ADMIN, &offered, cancel()).unwrap(),
        config(AuthoritySlot::Unset)
    );
    let pending = send(AUTHORITY, &config(holder(AUTHORITY)), nominate(SUCCESSOR)).unwrap();
    assert_eq!(
        send(ADMIN, &pending, cancel()).unwrap(),
        config(holder(AUTHORITY))
    );
    let replaced = send(ADMIN, &pending, nominate(STRANGER)).unwrap();
    assert_eq!(replaced.freeze.authority_slot().nominee(), Some(STRANGER));
}

#[test]
fn an_operation_of_the_freeze_role_sent_straight_to_the_config_is_refused_in_its_name() {
    for message in [nominate(SUCCESSOR), freeze()] {
        let refused = at(
            config_actor(),
            None,
            false,
            Some(&config(holder(AUTHORITY))),
            message,
        )
        .err();
        assert_eq!(
            refused,
            Some(Refusal::of::<Freeze>(AuthorityError::NotThroughSeat))
        );
        assert_eq!(
            refused.unwrap().text(),
            "lez-authority/freeze E122: a message that needs the freeze authority must go through the sender's own actor"
        );
    }
}

#[test]
fn before_the_admin_s_init_no_operation_of_the_freeze_role_runs() {
    for op in [
        nominate(AUTHORITY),
        freeze_authority_op(Op::Init),
        accept(),
        renounce(),
        freeze(),
        release(),
    ] {
        assert_eq!(
            send_via(ADMIN, None, None, op),
            Err(Refusal::of::<Admin>(AuthorityError::NotInitialized))
        );
    }
}

#[test]
fn the_freeze_authority_can_replace_or_cancel_the_admin_s_nomination() {
    let offered = send(ADMIN, &config(holder(AUTHORITY)), nominate(SUCCESSOR)).unwrap();
    let replaced = send(AUTHORITY, &offered, nominate(STRANGER)).unwrap();
    assert_eq!(replaced.freeze.authority_slot().nominee(), Some(STRANGER));
    let cancelled = send(AUTHORITY, &replaced, nominate(AccountId::default())).unwrap();
    assert_eq!(cancelled, config(holder(AUTHORITY)));
}

#[test]
fn the_freeze_slot_reads_back_its_flag_and_its_accounts() {
    for (frozen, frozen_accounts) in [
        (false, vec![]),
        (true, vec![]),
        (false, vec![AUTHORITY, STRANGER]),
        (true, vec![STRANGER]),
    ] {
        let slot = FreezeSlot {
            authority_slot: AuthoritySlot::Unset,
            frozen,
            frozen_accounts: frozen_accounts.clone(),
        };
        assert_eq!(slot.is_frozen(), frozen);
        assert_eq!(slot.frozen_accounts(), frozen_accounts);
    }
    let id = borsh::to_vec(&STRANGER).unwrap();
    let decoded: FreezeSlot = borsh::from_slice(&[&[0, 0, 1, 0, 0, 0][..], &id].concat()).unwrap();
    assert!(!decoded.is_frozen());
    assert_eq!(decoded.frozen_accounts(), [STRANGER]);
}

#[test]
fn a_freeze_operation_travels_as_the_freeze_role_s_own_operation() {
    let role = borsh::to_vec("freeze").unwrap();
    let id = borsh::to_vec(&STRANGER).unwrap();
    for (message, op) in [
        (freeze(), vec![0]),
        (release(), vec![1]),
        (freeze_account(STRANGER), [&[2][..], &id].concat()),
        (release_account(STRANGER), [&[3][..], &id].concat()),
    ] {
        let expected = [&[lez_authority::LIBRARY_TAG, 0][..], &role, &[4], &op].concat();
        assert_eq!(borsh::to_vec(&message).unwrap(), expected);
    }
}

#[test]
fn the_freeze_authority_freezes_and_releases_the_program() {
    let appointed = || {
        let mut config = config(AuthoritySlot::Held {
            holder: Holder {
                account: AUTHORITY,
                via: None,
            },
            nominee: Some(SUCCESSOR),
        });
        config.freeze.frozen_accounts = vec![STRANGER];
        config
    };
    let frozen_program = send(AUTHORITY, &appointed(), freeze()).unwrap();
    assert_eq!(frozen_program, frozen(appointed()));
    assert!(frozen_program.freeze.is_frozen());
    assert_eq!(send(AUTHORITY, &frozen_program, release()), Ok(appointed()));
}

#[test]
fn only_the_freeze_authority_freezes_and_releases() {
    let not_holder = Refusal::of::<Freeze>(AuthorityError::NotHolder);
    let pending = || send(AUTHORITY, &config(holder(AUTHORITY)), nominate(SUCCESSOR)).unwrap();
    for anyone in [ADMIN, STRANGER, SUCCESSOR] {
        for pre in [pending(), frozen(with_frozen(pending(), &[STRANGER]))] {
            for op in [
                freeze(),
                release(),
                freeze_account(SUCCESSOR),
                freeze_account(STRANGER),
                freeze_account(AccountId::default()),
                release_account(STRANGER),
            ] {
                assert_eq!(send(anyone, &pre, op), Err(not_holder.clone()));
            }
        }
    }
    assert_eq!(
        not_holder.text(),
        "lez-authority/freeze E103: the signing account is not the freeze authority"
    );
}

#[test]
fn with_no_freeze_authority_nobody_freezes_or_releases() {
    let nobody = Refusal::of::<Freeze>(AuthorityError::NotInstalled);
    for slot in [
        AuthoritySlot::Unset,
        AuthoritySlot::Offered { nominee: AUTHORITY },
    ] {
        for anyone in [ADMIN, AUTHORITY] {
            let frozen_all = frozen(with_frozen(config(slot.clone()), &[STRANGER]));
            for pre in [config(slot.clone()), frozen_all] {
                for op in [
                    freeze(),
                    release(),
                    freeze_account(SUCCESSOR),
                    freeze_account(STRANGER),
                    freeze_account(AccountId::default()),
                    release_account(STRANGER),
                ] {
                    assert_eq!(send(anyone, &pre, op), Err(nobody.clone()));
                }
            }
        }
    }
    assert_eq!(
        nobody.text(),
        "lez-authority/freeze E131: there is no freeze authority"
    );
}

#[test]
fn a_freeze_operation_that_changes_nothing_is_refused() {
    assert_eq!(
        send(AUTHORITY, &frozen(config(holder(AUTHORITY))), freeze()),
        Err(Refusal::of::<Freeze>(FreezeError::AlreadyFrozen.into()))
    );
    assert_eq!(
        send(AUTHORITY, &config(holder(AUTHORITY)), release()),
        Err(Refusal::of::<Freeze>(FreezeError::NotFrozen.into()))
    );
    let one_frozen = with_frozen(config(holder(AUTHORITY)), &[STRANGER]);
    assert_eq!(
        send(AUTHORITY, &one_frozen, freeze_account(STRANGER)),
        Err(Refusal::of::<Freeze>(
            FreezeError::AccountAlreadyFrozen.into()
        ))
    );
    assert_eq!(
        send(AUTHORITY, &one_frozen, release_account(SUCCESSOR)),
        Err(Refusal::of::<Freeze>(FreezeError::AccountNotFrozen.into()))
    );
    let three_frozen = with_frozen(config(holder(AUTHORITY)), &[AUTHORITY, STRANGER, ADMIN]);
    for account in [AUTHORITY, STRANGER, ADMIN] {
        assert_eq!(
            send(AUTHORITY, &three_frozen, freeze_account(account)),
            Err(Refusal::of::<Freeze>(
                FreezeError::AccountAlreadyFrozen.into()
            ))
        );
    }
    for account in [SUCCESSOR, ELSEWHERE] {
        assert_eq!(
            send(AUTHORITY, &three_frozen, release_account(account)),
            Err(Refusal::of::<Freeze>(FreezeError::AccountNotFrozen.into()))
        );
    }
}

#[test]
fn the_freeze_authority_freezes_and_releases_accounts_kept_sorted() {
    let mut state = valued(frozen(config(holder(AUTHORITY))), 7);
    let mut expected = Vec::new();
    for account in [STRANGER, AUTHORITY, ELSEWHERE, SUCCESSOR, ADMIN] {
        state = send(AUTHORITY, &state, freeze_account(account)).unwrap();
        expected.push(account);
        expected.sort();
        assert_eq!(state.freeze.frozen_accounts(), expected);
    }
    assert_eq!(
        state.freeze.frozen_accounts(),
        [AUTHORITY, SUCCESSOR, STRANGER, ELSEWHERE, ADMIN]
    );
    for account in [SUCCESSOR, ADMIN, AUTHORITY, ELSEWHERE, STRANGER] {
        state = send(AUTHORITY, &state, release_account(account)).unwrap();
        expected.retain(|frozen| *frozen != account);
        assert_eq!(state.freeze.frozen_accounts(), expected);
    }
    assert_eq!(state, valued(frozen(config(holder(AUTHORITY))), 7));
}

#[test]
fn the_frozen_accounts_follow_the_account_id_s_order() {
    let mut state = config(holder(AUTHORITY));
    for account in [id(3, 1), id(1, 9), id(2, 5), id(1, 5)] {
        state = send(AUTHORITY, &state, freeze_account(account)).unwrap();
    }
    assert_eq!(
        state.freeze.frozen_accounts(),
        [id(1, 5), id(1, 9), id(2, 5), id(3, 1)]
    );
    state = send(AUTHORITY, &state, release_account(id(1, 9))).unwrap();
    assert_eq!(
        state.freeze.frozen_accounts(),
        [id(1, 5), id(2, 5), id(3, 1)]
    );
    for account in [id(3, 1), id(1, 5), id(2, 5)] {
        state = send(AUTHORITY, &state, release_account(account)).unwrap();
    }
    assert_eq!(state, config(holder(AUTHORITY)));
}

#[test]
fn freezing_an_account_leaves_the_program_s_flag() {
    for pre in [config(holder(AUTHORITY)), frozen(config(holder(AUTHORITY)))] {
        let one_frozen = send(AUTHORITY, &pre, freeze_account(STRANGER)).unwrap();
        assert_eq!(one_frozen, with_frozen(pre.clone(), &[STRANGER]));
        assert_eq!(
            send(AUTHORITY, &one_frozen, release_account(STRANGER)),
            Ok(pre)
        );
    }
}

#[test]
fn freezing_the_all_zero_id_is_refused_and_the_config_s_own_id_is_not() {
    let appointed = config(holder(AUTHORITY));
    assert_eq!(
        send(AUTHORITY, &appointed, freeze_account(AccountId::default())),
        Err(Refusal::of::<Freeze>(FreezeError::ZeroAccount.into()))
    );
    assert_eq!(
        send(AUTHORITY, &appointed, release_account(AccountId::default())),
        Err(Refusal::of::<Freeze>(FreezeError::AccountNotFrozen.into()))
    );
    let own = config_actor().account_id;
    assert_eq!(
        send(AUTHORITY, &appointed, freeze_account(own)),
        Ok(with_frozen(appointed.clone(), &[own]))
    );
}

#[test]
fn a_freeze_outlasts_its_freeze_authority_until_the_next_one_releases_it() {
    let frozen_program = send(AUTHORITY, &config(holder(AUTHORITY)), freeze()).unwrap();
    let stepped_down = send(AUTHORITY, &frozen_program, renounce()).unwrap();
    assert_eq!(stepped_down, frozen(config(AuthoritySlot::Unset)));
    assert_eq!(straight(&stepped_down, Message::Set(1)), Err(e217()));
    assert_eq!(
        send(AUTHORITY, &stepped_down, release()),
        Err(Refusal::of::<Freeze>(AuthorityError::NotInstalled))
    );
    let offered = send(ADMIN, &stepped_down, nominate(SUCCESSOR)).unwrap();
    assert_eq!(straight(&offered, Message::Set(1)), Err(e217()));
    let appointed = send(SUCCESSOR, &offered, accept()).unwrap();
    assert_eq!(appointed, frozen(config(holder(SUCCESSOR))));
    assert_eq!(
        send(SUCCESSOR, &appointed, release()),
        Ok(config(holder(SUCCESSOR)))
    );
}

#[test]
fn a_pda_freeze_authority_freezes_and_releases_through_its_program_only() {
    let pda = AccountId::for_public_pda(&GOVERNANCE, &PdaSeed::new([9; 32]));
    let governed = || {
        config(AuthoritySlot::held(Holder {
            account: pda,
            via: Some(GOVERNANCE),
        }))
    };
    let frozen_program = send_via(pda, Some(GOVERNANCE), Some(&governed()), freeze()).unwrap();
    assert_eq!(frozen_program, frozen(governed()));
    for via in [None, Some(ELSEWHERE)] {
        assert_eq!(
            send_via(pda, via, Some(&frozen_program), release()),
            Err(Refusal::of::<Freeze>(AuthorityError::ViaNotAllowed))
        );
    }
    assert_eq!(
        send_via(pda, Some(GOVERNANCE), Some(&frozen_program), release()),
        Ok(governed())
    );
    let one_frozen = with_frozen(governed(), &[STRANGER]);
    for via in [None, Some(ELSEWHERE)] {
        for (pre, op) in [
            (governed(), freeze_account(STRANGER)),
            (one_frozen.clone(), release_account(STRANGER)),
        ] {
            assert_eq!(
                send_via(pda, via, Some(&pre), op),
                Err(Refusal::of::<Freeze>(AuthorityError::ViaNotAllowed))
            );
        }
    }
    assert_eq!(
        send_via(
            pda,
            Some(GOVERNANCE),
            Some(&governed()),
            freeze_account(STRANGER)
        ),
        Ok(one_frozen.clone())
    );
    assert_eq!(
        send_via(
            pda,
            Some(GOVERNANCE),
            Some(&one_frozen),
            release_account(STRANGER)
        ),
        Ok(governed())
    );
}

#[test]
fn a_freeze_operation_that_does_not_decode_is_refused_in_the_freeze_s_name() {
    for sender in [AUTHORITY, STRANGER] {
        let id = borsh::to_vec(&STRANGER).unwrap();
        for bytes in [
            Vec::new(),
            vec![9],
            vec![0, 0],
            vec![2],
            [&[3][..], &id[1..]].concat(),
            [&[2][..], &id, &[0]].concat(),
        ] {
            assert_eq!(
                send(
                    sender,
                    &config(holder(AUTHORITY)),
                    freeze_authority_op(Op::Own(bytes))
                ),
                Err(Refusal::of::<Freeze>(AuthorityError::MessageDoesNotDecode))
            );
        }
    }
}

#[test]
fn a_freeze_standing_when_the_admin_renounces_is_released_by_the_freeze_authority() {
    let without_admin = |freeze| Config {
        admin: AuthoritySlot::Renounced,
        ..config(freeze)
    };
    let frozen_program = frozen(config(holder(AUTHORITY)));
    let renounced = send(ADMIN, &frozen_program, admin_op(Op::Renounce)).unwrap();
    assert_eq!(renounced, frozen(without_admin(holder(AUTHORITY))));
    assert_eq!(straight(&renounced, Message::Set(1)), Err(e217()));
    let released = send(AUTHORITY, &renounced, release()).unwrap();
    assert_eq!(released, without_admin(holder(AUTHORITY)));
    assert_eq!(
        send(AUTHORITY, &released, freeze()),
        Ok(frozen(without_admin(holder(AUTHORITY))))
    );
}

#[test]
fn a_freeze_refuses_every_program_message_at_the_config() {
    let frozen_program = || frozen(config(holder(AUTHORITY)));
    assert_eq!(straight(&frozen_program(), Message::Set(1)), Err(e217()));
    assert_eq!(straight(&frozen_program(), Message::Touch(4)), Err(e217()));
    assert_eq!(
        send(ADMIN, &frozen_program(), Message::AdminSet(1).into()),
        Err(e217())
    );
}

#[test]
fn an_exempt_message_runs_through_a_freeze() {
    let frozen_program = || frozen(config(holder(AUTHORITY)));
    assert_eq!(
        straight(&frozen_program(), Message::ExemptSet(5)),
        Ok(valued(frozen_program(), 5))
    );
    assert_eq!(
        send(ADMIN, &frozen_program(), Message::AdminExemptSet(6).into()),
        Ok(valued(frozen_program(), 6))
    );
}

#[test]
fn an_exempt_message_still_needs_its_holder_and_cannot_lift_the_freeze() {
    let frozen_program = frozen(config(holder(AUTHORITY)));
    assert_eq!(
        send(STRANGER, &frozen_program, Message::AdminExemptSet(6).into()),
        Err(Refusal::of::<Admin>(AuthorityError::NotHolder))
    );
    assert_eq!(
        straight(&frozen_program, Message::ExemptTouch(1)),
        Err(Refusal::of::<Freeze>(AuthorityError::SlotChanged))
    );
}

#[test]
fn the_admin_s_check_comes_before_the_freeze() {
    assert_eq!(
        send(
            STRANGER,
            &frozen(config(holder(AUTHORITY))),
            Message::AdminSet(1).into()
        ),
        Err(Refusal::of::<Admin>(AuthorityError::NotHolder))
    );
}

#[test]
fn every_role_operation_runs_through_a_freeze() {
    let frozen_program = frozen(config(holder(AUTHORITY)));
    let offered = send(
        ADMIN,
        &frozen_program,
        admin_op(Op::Nominate { next: SUCCESSOR }),
    )
    .unwrap();
    let handed_over = send(SUCCESSOR, &offered, admin_op(Op::Accept { pda_seed: None })).unwrap();
    let under_successor = |freeze| Config {
        admin: holder(SUCCESSOR),
        ..config(freeze)
    };
    assert_eq!(handed_over, frozen(under_successor(holder(AUTHORITY))));
    let nominated = send(SUCCESSOR, &handed_over, nominate(STRANGER)).unwrap();
    let replaced = send(STRANGER, &nominated, accept()).unwrap();
    assert_eq!(replaced, frozen(under_successor(holder(STRANGER))));
    assert_eq!(
        send(STRANGER, &replaced, release()),
        Ok(under_successor(holder(STRANGER)))
    );
    let one_frozen = send(STRANGER, &replaced, freeze_account(AUTHORITY)).unwrap();
    assert_eq!(one_frozen, with_frozen(replaced.clone(), &[AUTHORITY]));
    assert_eq!(
        send(STRANGER, &one_frozen, release_account(AUTHORITY)),
        Ok(replaced.clone())
    );
    assert_eq!(
        send(STRANGER, &replaced, renounce()),
        Ok(frozen(under_successor(AuthoritySlot::Unset)))
    );
}

#[test]
fn after_a_release_every_message_runs_again() {
    let released = send(AUTHORITY, &frozen(config(holder(AUTHORITY))), release()).unwrap();
    assert_eq!(
        straight(&released, Message::Set(3)),
        Ok(valued(config(holder(AUTHORITY)), 3))
    );
    assert_eq!(
        send(ADMIN, &released, Message::AdminSet(4).into()),
        Ok(valued(config(holder(AUTHORITY)), 4))
    );
}

#[test]
fn before_the_admin_s_init_an_exempt_message_at_the_config_is_refused() {
    assert_eq!(
        at(
            config_actor(),
            None,
            false,
            None,
            Message::ExemptSet(1).into()
        )
        .err(),
        Some(Refusal::of::<Admin>(AuthorityError::NotInitialized))
    );
}

#[test]
fn a_freeze_left_with_no_admin_and_no_freeze_authority_stands_for_good() {
    let abandoned = || {
        frozen(Config {
            admin: AuthoritySlot::Renounced,
            ..config(AuthoritySlot::Unset)
        })
    };
    let renounced = send(
        ADMIN,
        &frozen(config(holder(AUTHORITY))),
        admin_op(Op::Renounce),
    )
    .unwrap();
    assert_eq!(send(AUTHORITY, &renounced, renounce()), Ok(abandoned()));
    assert_eq!(straight(&abandoned(), Message::Set(1)), Err(e217()));
    assert_eq!(
        send(AUTHORITY, &abandoned(), release()),
        Err(Refusal::of::<Freeze>(AuthorityError::NotInstalled))
    );
    assert_eq!(
        straight(&abandoned(), Message::ExemptSet(2)),
        Ok(valued(abandoned(), 2))
    );
}

#[test]
fn the_freeze_authority_s_own_program_message_is_frozen_too() {
    assert_eq!(
        send(
            AUTHORITY,
            &frozen(config(holder(AUTHORITY))),
            Message::AuthoritySet(1).into()
        ),
        Err(e217())
    );
    assert_eq!(
        send(
            AUTHORITY,
            &config(holder(AUTHORITY)),
            Message::AuthoritySet(1).into()
        ),
        Ok(valued(config(holder(AUTHORITY)), 1))
    );
}

fn actor(account: AccountId) -> Actor {
    Actor::new(account, PROGRAM)
}

/// `message` handled at `account`'s actor, sent by `from`, then any request for clearance it
/// adds, answered at the config as `pre` holds it. The actor's response.
fn elsewhere(
    account: AccountId,
    from: Option<Actor>,
    authorized: bool,
    pre: &Config,
    message: Message,
) -> Result<Response, Refusal> {
    let response = at(actor(account), from, authorized, None, message.into())?;
    for sent in response
        .sync_messages
        .iter()
        .filter(|sent| sent.to == config_actor())
    {
        let request: Wire<Message> = borsh::from_slice(&sent.message).unwrap();
        let answer = at(
            config_actor(),
            Some(actor(account)),
            false,
            Some(pre),
            request,
        )?;
        assert_eq!(answer.post_state, None);
    }
    Ok(response)
}

/// The requests for clearance in `response`.
fn requests(response: &Response) -> Vec<Clearance> {
    response
        .sync_messages
        .iter()
        .filter(|sent| sent.to == config_actor())
        .map(|sent| match borsh::from_slice(&sent.message).unwrap() {
            Wire::<Message>::Library(LibraryMessage::Clearance(request)) => request,
            _ => panic!("not a request for clearance"),
        })
        .collect()
}

#[test]
fn a_freeze_reaches_a_message_at_another_actor_whoever_sent_it() {
    let frozen_program = frozen(config(holder(AUTHORITY)));
    for from in senders() {
        for authorized in [true, false] {
            let at_stranger =
                |pre: &Config| elsewhere(STRANGER, from, authorized, pre, Message::Count(1));
            assert_eq!(at_stranger(&frozen_program).err(), Some(e217()));
            let counted = at_stranger(&config(holder(AUTHORITY))).unwrap();
            assert_eq!(counted.post_state.as_deref(), Some(&[1u8][..]));
            assert_eq!(requests(&counted), [Clearance { authorized }]);
        }
    }
}

/// Every sender a message at another actor can have: a client, another actor of the program, an
/// actor of another program, and the config.
fn senders() -> [Option<Actor>; 4] {
    [
        None,
        Some(actor(SUCCESSOR)),
        Some(Actor::new(SUCCESSOR, ELSEWHERE)),
        Some(config_actor()),
    ]
}

#[test]
fn an_exempt_message_handled_at_another_actor_runs_through_a_freeze() {
    for from in senders() {
        let counted = elsewhere(
            STRANGER,
            from,
            true,
            &frozen(config(holder(AUTHORITY))),
            Message::ExemptCount(1),
        )
        .unwrap();
        assert_eq!(counted.post_state.as_deref(), Some(&[1u8][..]));
        assert!(requests(&counted).is_empty());
    }
}

#[test]
fn a_freeze_reaches_a_message_the_config_sends_on() {
    let credit = |pre: &Config| {
        elsewhere(
            SUCCESSOR,
            Some(config_actor()),
            false,
            pre,
            Message::Credit(2),
        )
    };
    assert_eq!(
        credit(&frozen(config(holder(AUTHORITY)))).err(),
        Some(e217())
    );
    assert_eq!(
        credit(&config(holder(AUTHORITY)))
            .unwrap()
            .post_state
            .as_deref(),
        Some(&[2u8][..])
    );
}

#[test]
fn after_a_release_a_message_at_another_actor_runs_again() {
    let released = send(AUTHORITY, &frozen(config(holder(AUTHORITY))), release()).unwrap();
    let counted = elsewhere(STRANGER, None, false, &released, Message::Count(1)).unwrap();
    assert_eq!(counted.post_state.as_deref(), Some(&[1u8][..]));
    assert_eq!(requests(&counted), [Clearance { authorized: false }]);
}

#[test]
fn before_the_admin_s_init_no_freezable_message_at_another_actor_runs() {
    let response = at(actor(STRANGER), None, true, None, Message::Count(1).into()).unwrap();
    assert_eq!(requests(&response), [Clearance { authorized: true }]);
    let request =
        Wire::<Message>::Library(LibraryMessage::Clearance(Clearance { authorized: true }));
    assert_eq!(
        at(config_actor(), Some(actor(STRANGER)), false, None, request).err(),
        Some(Refusal::of::<Admin>(AuthorityError::NotInitialized))
    );
}

#[test]
fn before_the_admin_s_init_an_exempt_message_at_another_actor_runs() {
    let response = at(
        actor(STRANGER),
        None,
        true,
        None,
        Message::ExemptCount(1).into(),
    )
    .unwrap();
    assert_eq!(response.post_state.as_deref(), Some(&[1u8][..]));
    assert!(requests(&response).is_empty());
}

#[test]
fn a_seat_forwards_a_role_operation_and_a_gated_message_with_no_request_for_clearance() {
    for (sender, message) in [(AUTHORITY, release()), (ADMIN, Message::AdminSet(1).into())] {
        let hop = at(actor(sender), None, true, None, message).unwrap();
        let [sent] = hop.sync_messages.as_slice() else {
            panic!("one message from the seat");
        };
        assert!(matches!(
            borsh::from_slice::<Wire<Message>>(&sent.message).unwrap(),
            Wire::Library(LibraryMessage::Forwarded(_))
        ));
    }
}
