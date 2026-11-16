use lee_core::account::{Actor, ActorState};
use lee_core::program::{PdaSeed, ReceiveInput, Response};
use lez_authority::try_guard;

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

#[derive(Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, HasSlot)]
struct Config {
    #[slot(Admin)]
    admin: AuthoritySlot,
    #[slot(Freeze)]
    freeze: FreezeSlot,
}

#[derive(BorshSerialize, BorshDeserialize, Gated)]
enum Message {
    /// Changes one part of the freeze slot, or nothing for any other number.
    Touch(u8),
}

fn handle(input: &ReceiveInput, message: Message) -> Response {
    let Message::Touch(part) = message;
    let mut config: Config = borsh::from_slice(&input.pre_state).unwrap();
    match part {
        0 => *config.freeze.authority_slot_mut() = holder(STRANGER),
        1 => config.freeze.frozen = !config.freeze.frozen,
        2 => config.freeze.frozen_accounts.push(SUCCESSOR),
        3 => config.freeze.frozen_accounts.reverse(),
        _ => {}
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

/// `config` with the program frozen.
fn frozen(mut config: Config) -> Config {
    config.freeze.frozen = true;
    config
}

fn e208() -> Refusal {
    Refusal::of::<Freeze>(FreezeError::NotAdminOrFreezeAuthority.into())
}

#[test]
fn the_freeze_codes_are_stable() {
    let expected: [(FreezeError, u16); 3] = [
        (FreezeError::NotAdminOrFreezeAuthority, 208),
        (FreezeError::AlreadyFrozen, 211),
        (FreezeError::NotFrozen, 212),
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
    for (message, op) in [(freeze(), 0), (release(), 1)] {
        let expected = [&[lez_authority::LIBRARY_TAG, 0][..], &role, &[4, op]].concat();
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
        for pre in [pending(), frozen(pending())] {
            for op in [freeze(), release()] {
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
            for pre in [config(slot.clone()), frozen(config(slot.clone()))] {
                for op in [freeze(), release()] {
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
}

#[test]
fn a_freeze_outlasts_its_freeze_authority_until_the_next_one_releases_it() {
    let frozen_program = send(AUTHORITY, &config(holder(AUTHORITY)), freeze()).unwrap();
    let stepped_down = send(AUTHORITY, &frozen_program, renounce()).unwrap();
    assert_eq!(stepped_down, frozen(config(AuthoritySlot::Unset)));
    assert_eq!(
        send(AUTHORITY, &stepped_down, release()),
        Err(Refusal::of::<Freeze>(AuthorityError::NotInstalled))
    );
    let offered = send(ADMIN, &stepped_down, nominate(SUCCESSOR)).unwrap();
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
}

#[test]
fn a_freeze_operation_that_does_not_decode_is_refused_in_the_freeze_s_name() {
    for sender in [AUTHORITY, STRANGER] {
        for bytes in [Vec::new(), vec![9], vec![0, 0]] {
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
    let released = send(AUTHORITY, &renounced, release()).unwrap();
    assert_eq!(released, without_admin(holder(AUTHORITY)));
    assert_eq!(
        send(AUTHORITY, &released, freeze()),
        Ok(frozen(without_admin(holder(AUTHORITY))))
    );
}
