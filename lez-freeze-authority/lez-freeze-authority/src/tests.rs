use std::collections::{BTreeMap, BTreeSet};

use lee_core::account::{Actor, ActorState};
use lee_core::program::{PdaSeed, ReceiveInput, Response};
use lez_authority::{Clearance, LibraryMessage, own_actor, try_guard};

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
    /// Writes its bytes as the state of the actor it reaches, away from the config.
    Write(Vec<u8>),
    #[exempt(Freeze)]
    ExemptWrite(Vec<u8>),
}

fn handle(input: &ReceiveInput, message: Message) -> Response {
    if let Message::Count(more) | Message::ExemptCount(more) | Message::Credit(more) = message {
        let count = input.pre_state.first().copied().unwrap_or(0);
        return Response::set_state(vec![count + more]);
    }
    if let Message::Write(state) | Message::ExemptWrite(state) = message {
        return Response::set_state(state);
    }
    let mut config: Config = borsh::from_slice(&input.pre_state).unwrap();
    match message {
        Message::Touch(part) | Message::ExemptTouch(part) => match part {
            0 => *config.freeze.authority_slot_mut() = holder(STRANGER),
            1 => config.freeze.frozen = !config.freeze.frozen,
            _ => {}
        },
        Message::Set(value)
        | Message::AdminSet(value)
        | Message::AuthoritySet(value)
        | Message::ExemptSet(value)
        | Message::AdminExemptSet(value) => config.value = value,
        Message::Count(_)
        | Message::ExemptCount(_)
        | Message::Credit(_)
        | Message::Write(_)
        | Message::ExemptWrite(_) => unreachable!(),
    }
    Response::set_state(borsh::to_vec(&config).unwrap())
}

fn config_actor() -> Actor {
    Actor::new(config_account(PROGRAM, &SEED), PROGRAM)
}

fn actor(account: AccountId) -> Actor {
    Actor::new(account, PROGRAM)
}

fn marker(account: AccountId) -> Actor {
    marker_actor(config_actor(), account)
}

/// The guard at `receiver` over `state`, with nothing it sends delivered.
fn guard_at(
    receiver: Actor,
    from: Option<Actor>,
    authorized: bool,
    state: Vec<u8>,
    message: Vec<u8>,
) -> Result<Response, Refusal> {
    let wire: Wire<Message> = borsh::from_slice(&message).unwrap();
    let input = ReceiveInput {
        receiver,
        from,
        is_authorized: authorized,
        pre_state: ActorState::from(state),
        message,
    };
    try_guard::<(Admin, Freeze), Config, Message>(&input, &SEED, InitGate::Deployer, wire, handle)
}

/// [`guard_at`] over the config `pre` holds, or none.
fn at(
    receiver: Actor,
    from: Option<Actor>,
    authorized: bool,
    pre: Option<&Config>,
    message: Wire<Message>,
) -> Result<Response, Refusal> {
    let state = pre.map(|config| borsh::to_vec(config).unwrap());
    guard_at(
        receiver,
        from,
        authorized,
        state.unwrap_or_default(),
        borsh::to_vec(&message).unwrap(),
    )
}

/// Every actor's state in one transaction, as the runtime keeps it.
type States = BTreeMap<Actor, Vec<u8>>;

/// Runs `message` at `to` as the runtime does: the guard over the actor's state, the state it
/// writes, then each message it sends, depth first. `signers` signed the transaction. The
/// guard's answer at `to`.
fn execute(
    states: &mut States,
    to: Actor,
    from: Option<Actor>,
    signers: &[AccountId],
    message: Vec<u8>,
) -> Result<Response, Refusal> {
    let state = states.get(&to).cloned().unwrap_or_default();
    let response = guard_at(to, from, signers.contains(&to.account_id), state, message)?;
    if let Some(post) = &response.post_state {
        states.insert(to, post.to_vec());
    }
    for sent in &response.sync_messages {
        let _answer = execute(states, sent.to, Some(to), signers, sent.message.clone())?;
    }
    Ok(response)
}

/// `message` at `to` from `from`, signed by `signers`, with all it sends. The states it leaves,
/// or the refusal that refuses all of it.
fn run(
    pre: &States,
    to: Actor,
    from: Option<Actor>,
    signers: &[AccountId],
    message: Wire<Message>,
) -> Result<States, Refusal> {
    let mut states = pre.clone();
    let _answer = execute(
        &mut states,
        to,
        from,
        signers,
        borsh::to_vec(&message).unwrap(),
    )?;
    Ok(states)
}

/// From `sender`'s seat, reached at top level or through `via`, signed by `sender`.
fn transact_via(
    sender: AccountId,
    via: Option<AccountId>,
    pre: &States,
    message: Wire<Message>,
) -> Result<States, Refusal> {
    let from = via.map(|program| Actor::new(program, program));
    run(pre, actor(sender), from, &[sender], message)
}

fn transact(sender: AccountId, pre: &States, message: Wire<Message>) -> Result<States, Refusal> {
    transact_via(sender, None, pre, message)
}

/// [`transact_via`] in a program with nothing marked, or before the admin's `Init`. The config
/// it leaves.
fn send_via(
    sender: AccountId,
    via: Option<AccountId>,
    pre: Option<&Config>,
    message: Wire<Message>,
) -> Result<Config, Refusal> {
    let pre = pre.map_or_else(States::new, world);
    transact_via(sender, via, &pre, message).map(|states| config_of(&states))
}

fn send(sender: AccountId, pre: &Config, message: Wire<Message>) -> Result<Config, Refusal> {
    send_via(sender, None, Some(pre), message)
}

/// The states of a program whose config is `config`, with nothing marked.
fn world(config: &Config) -> States {
    States::from([(config_actor(), borsh::to_vec(config).unwrap())])
}

/// The states of a program whose config is `config` and whose `accounts` are frozen.
fn with_frozen(config: &Config, accounts: &[AccountId]) -> States {
    let mut states = world(config);
    for &account in accounts {
        states.insert(marker(account), MARK.to_vec());
    }
    states
}

/// `states` with `account`'s marker holding `state`.
fn with_marker(mut states: States, account: AccountId, state: &[u8]) -> States {
    states.insert(marker(account), state.to_vec());
    states
}

/// The config `states` hold.
fn config_of(states: &States) -> Config {
    borsh::from_slice(&states[&config_actor()]).unwrap()
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

/// States a marker holds that are not the mark, the last one as long as it.
fn strays() -> [Vec<u8>; 5] {
    let mut near = MARK;
    near[31] = 1;
    [
        vec![1],
        vec![0; 64],
        MARK[..31].to_vec(),
        [&MARK[..], &[0]].concat(),
        near.to_vec(),
    ]
}

/// `message` sent straight to the config. The config it leaves.
fn straight(pre: &Config, message: Message) -> Result<Config, Refusal> {
    run(&world(pre), config_actor(), None, &[], message.into()).map(|states| config_of(&states))
}

fn e217() -> Refusal {
    Refusal::of::<Freeze>(FreezeError::Frozen.into())
}

fn e208() -> Refusal {
    Refusal::of::<Freeze>(FreezeError::NotAdminOrFreezeAuthority.into())
}

fn e213() -> Refusal {
    Refusal::of::<Freeze>(FreezeError::AccountAlreadyFrozen.into())
}

fn e214() -> Refusal {
    Refusal::of::<Freeze>(FreezeError::AccountNotFrozen.into())
}

fn e218() -> Refusal {
    Refusal::of::<Freeze>(FreezeError::AccountFrozen.into())
}

fn e227() -> Refusal {
    Refusal::of::<Freeze>(FreezeError::AtMarker.into())
}

fn e228() -> Refusal {
    Refusal::of::<Freeze>(FreezeError::WritesMark.into())
}

#[test]
fn the_freeze_codes_are_stable() {
    let expected: [(FreezeError, u16); 10] = [
        (FreezeError::NotAdminOrFreezeAuthority, 208),
        (FreezeError::AlreadyFrozen, 211),
        (FreezeError::NotFrozen, 212),
        (FreezeError::AccountAlreadyFrozen, 213),
        (FreezeError::AccountNotFrozen, 214),
        (FreezeError::Frozen, 217),
        (FreezeError::AccountFrozen, 218),
        (FreezeError::ZeroAccount, 225),
        (FreezeError::AtMarker, 227),
        (FreezeError::WritesMark, 228),
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
        e213().text(),
        "lez-freeze-authority/freeze E213: the account is already frozen"
    );
    assert_eq!(
        e214().text(),
        "lez-freeze-authority/freeze E214: the account is not frozen"
    );
    assert_eq!(
        e217().text(),
        "lez-freeze-authority/freeze E217: the program is frozen"
    );
    assert_eq!(
        e218().text(),
        "lez-freeze-authority/freeze E218: the signing account is frozen"
    );
    assert_eq!(
        Refusal::of::<Freeze>(FreezeError::ZeroAccount.into()).text(),
        "lez-freeze-authority/freeze E225: the all-zero id names no account"
    );
    assert_eq!(
        e227().text(),
        "lez-freeze-authority/freeze E227: the actor is a frozen account's marker"
    );
    assert_eq!(
        e228().text(),
        "lez-freeze-authority/freeze E228: the new state starts with the freeze's mark"
    );
}

#[test]
fn the_freeze_slot_encodes_its_holder_part_and_the_flag() {
    assert_eq!(borsh::to_vec(&FreezeSlot::default()).unwrap(), [0, 0]);
    let slot = FreezeSlot {
        authority_slot: holder(STRANGER),
        frozen: true,
    };
    let holder_part = borsh::to_vec(&holder(STRANGER)).unwrap();
    assert_eq!(
        borsh::to_vec(&slot).unwrap(),
        [&holder_part[..], &[1]].concat()
    );
}

#[test]
fn the_admin_s_init_creates_the_freeze_slot_unset_with_nothing_frozen() {
    let created = transact(ADMIN, &States::new(), admin_op(Op::Init)).unwrap();
    assert_eq!(created, world(&config(AuthoritySlot::Unset)));
    assert_eq!(config_of(&created).freeze, FreezeSlot::default());
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
    let pre = with_frozen(&frozen(config(holder(AUTHORITY))), &[STRANGER]);
    assert_eq!(
        transact(ADMIN, &pre, renounce()),
        Ok(with_frozen(
            &frozen(config(AuthoritySlot::Unset)),
            &[STRANGER]
        ))
    );
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
    let pre = config(holder(AUTHORITY));
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
    assert_eq!(at_config(2), None);
    for part in 0..2 {
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
        freeze_account(STRANGER),
        release_account(STRANGER),
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
fn the_freeze_slot_reads_back_its_flag() {
    for frozen in [false, true] {
        let slot = FreezeSlot {
            authority_slot: AuthoritySlot::Unset,
            frozen,
        };
        assert_eq!(slot.is_frozen(), frozen);
    }
    let decoded: FreezeSlot = borsh::from_slice(&[0, 1]).unwrap();
    assert!(decoded.is_frozen());
    let decoded: FreezeSlot = borsh::from_slice(&[0, 0]).unwrap();
    assert!(!decoded.is_frozen());
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
    let appointed = config(AuthoritySlot::Held {
        holder: Holder {
            account: AUTHORITY,
            via: None,
        },
        nominee: Some(SUCCESSOR),
    });
    let pre = with_frozen(&appointed, &[STRANGER]);
    let frozen_program = transact(AUTHORITY, &pre, freeze()).unwrap();
    assert_eq!(
        frozen_program,
        with_frozen(&frozen(appointed.clone()), &[STRANGER])
    );
    assert!(config_of(&frozen_program).freeze.is_frozen());
    assert_eq!(transact(AUTHORITY, &frozen_program, release()), Ok(pre));
}

#[test]
fn only_the_freeze_authority_freezes_and_releases() {
    let not_holder = Refusal::of::<Freeze>(AuthorityError::NotHolder);
    let pending = || send(AUTHORITY, &config(holder(AUTHORITY)), nominate(SUCCESSOR)).unwrap();
    for anyone in [ADMIN, STRANGER, SUCCESSOR] {
        for pre in [
            world(&pending()),
            with_frozen(&frozen(pending()), &[STRANGER]),
        ] {
            for op in [
                freeze(),
                release(),
                freeze_account(SUCCESSOR),
                freeze_account(STRANGER),
                freeze_account(AccountId::default()),
                release_account(STRANGER),
            ] {
                assert_eq!(transact(anyone, &pre, op), Err(not_holder.clone()));
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
            let frozen_all = with_frozen(&frozen(config(slot.clone())), &[STRANGER]);
            for pre in [world(&config(slot.clone())), frozen_all] {
                for op in [
                    freeze(),
                    release(),
                    freeze_account(SUCCESSOR),
                    freeze_account(STRANGER),
                    freeze_account(AccountId::default()),
                    release_account(STRANGER),
                ] {
                    assert_eq!(transact(anyone, &pre, op), Err(nobody.clone()));
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
    let one_frozen = with_frozen(&config(holder(AUTHORITY)), &[STRANGER]);
    assert_eq!(
        transact(AUTHORITY, &one_frozen, freeze_account(STRANGER)),
        Err(e213())
    );
    assert_eq!(
        transact(AUTHORITY, &one_frozen, release_account(SUCCESSOR)),
        Err(e214())
    );
    let three_frozen = with_frozen(&config(holder(AUTHORITY)), &[AUTHORITY, STRANGER, ADMIN]);
    for account in [AUTHORITY, STRANGER, ADMIN] {
        assert_eq!(
            transact(AUTHORITY, &three_frozen, freeze_account(account)),
            Err(e213())
        );
    }
    for account in [SUCCESSOR, ELSEWHERE] {
        assert_eq!(
            transact(AUTHORITY, &three_frozen, release_account(account)),
            Err(e214())
        );
    }
}

#[test]
fn the_freeze_authority_freezes_and_releases_accounts_one_by_one() {
    let pre = valued(frozen(config(holder(AUTHORITY))), 7);
    let mut states = world(&pre);
    let mut expected = world(&pre);
    for account in [STRANGER, AUTHORITY, ELSEWHERE, SUCCESSOR, ADMIN] {
        states = transact(AUTHORITY, &states, freeze_account(account)).unwrap();
        expected.insert(marker(account), MARK.to_vec());
        assert_eq!(states, expected);
    }
    for account in [SUCCESSOR, ADMIN, AUTHORITY, ELSEWHERE, STRANGER] {
        states = transact(AUTHORITY, &states, release_account(account)).unwrap();
        expected.insert(marker(account), Vec::new());
        assert_eq!(states, expected);
    }
}

#[test]
fn an_account_s_marker_is_the_freeze_s_own_actor_for_its_id() {
    let accounts = [
        id(1, 5),
        id(1, 9),
        id(2, 5),
        AUTHORITY,
        config_actor().account_id,
    ];
    let markers: BTreeSet<Actor> = accounts.iter().map(|&account| marker(account)).collect();
    assert_eq!(markers.len(), accounts.len());
    for account in accounts {
        assert_eq!(
            marker(account),
            own_actor::<Freeze>(config_actor(), account.value())
        );
        assert_ne!(marker(account), actor(account));
    }
    let appointed = config(holder(AUTHORITY));
    let one_frozen = transact(AUTHORITY, &world(&appointed), freeze_account(id(1, 5))).unwrap();
    assert_eq!(one_frozen, with_frozen(&appointed, &[id(1, 5)]));
    for account in [id(1, 9), id(2, 5)] {
        let counted =
            elsewhere(actor(account), None, true, &one_frozen, Message::Count(1)).unwrap();
        assert_eq!(counted.post_state.as_deref(), Some(&[1u8][..]));
    }
}

#[test]
fn the_mark_is_fixed() {
    assert_eq!(&MARK[..28], b"lez-freeze-authority/mark/v1");
    assert_eq!(MARK[28..], [0; 4]);
}

#[test]
fn freezing_an_account_leaves_the_program_s_flag() {
    for pre in [config(holder(AUTHORITY)), frozen(config(holder(AUTHORITY)))] {
        let one_frozen = transact(AUTHORITY, &world(&pre), freeze_account(STRANGER)).unwrap();
        assert_eq!(one_frozen, with_frozen(&pre, &[STRANGER]));
        assert_eq!(
            transact(AUTHORITY, &one_frozen, release_account(STRANGER)),
            Ok(with_marker(world(&pre), STRANGER, &[]))
        );
    }
}

#[test]
fn freezing_an_account_writes_the_mark_over_stray_state() {
    let appointed = config(holder(AUTHORITY));
    for stray in [vec![Vec::new()], strays().to_vec()].concat() {
        let pre = with_marker(world(&appointed), STRANGER, &stray);
        assert_eq!(
            transact(AUTHORITY, &pre, freeze_account(STRANGER)),
            Ok(with_frozen(&appointed, &[STRANGER]))
        );
        assert_eq!(
            transact(AUTHORITY, &pre, release_account(STRANGER)),
            Err(e214())
        );
    }
}

#[test]
fn stray_state_at_a_caller_s_marker_does_not_freeze_it() {
    let appointed = config(holder(AUTHORITY));
    for stray in strays() {
        let pre = with_marker(world(&appointed), ADMIN, &stray);
        assert_eq!(
            transact(ADMIN, &pre, Message::AdminSet(1).into()),
            Ok(with_marker(
                world(&valued(appointed.clone(), 1)),
                ADMIN,
                &stray
            ))
        );
        let pre = with_marker(world(&appointed), STRANGER, &stray);
        let mut counted = pre.clone();
        counted.insert(actor(STRANGER), vec![1]);
        assert_eq!(
            run(
                &pre,
                actor(STRANGER),
                None,
                &[STRANGER],
                Message::Count(1).into()
            ),
            Ok(counted)
        );
    }
}

#[test]
fn releasing_an_account_leaves_its_marker_empty() {
    let appointed = config(holder(AUTHORITY));
    let released = transact(
        AUTHORITY,
        &with_frozen(&appointed, &[STRANGER]),
        release_account(STRANGER),
    )
    .unwrap();
    assert_eq!(released, with_marker(world(&appointed), STRANGER, &[]));
    assert_eq!(
        transact(AUTHORITY, &released, release_account(STRANGER)),
        Err(e214())
    );
    assert_eq!(
        transact(AUTHORITY, &released, freeze_account(STRANGER)),
        Ok(with_frozen(&appointed, &[STRANGER]))
    );
}

#[test]
fn freezing_the_all_zero_id_is_refused_and_the_config_s_own_id_is_not() {
    let appointed = config(holder(AUTHORITY));
    assert_eq!(
        transact(
            AUTHORITY,
            &world(&appointed),
            freeze_account(AccountId::default())
        ),
        Err(Refusal::of::<Freeze>(FreezeError::ZeroAccount.into()))
    );
    assert_eq!(
        transact(
            AUTHORITY,
            &world(&appointed),
            release_account(AccountId::default())
        ),
        Err(e214())
    );
    let own = config_actor().account_id;
    assert_eq!(
        transact(AUTHORITY, &world(&appointed), freeze_account(own)),
        Ok(with_frozen(&appointed, &[own]))
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
    let one_frozen = with_frozen(&governed(), &[STRANGER]);
    for via in [None, Some(ELSEWHERE)] {
        for (pre, op) in [
            (world(&governed()), freeze_account(STRANGER)),
            (one_frozen.clone(), release_account(STRANGER)),
        ] {
            assert_eq!(
                transact_via(pda, via, &pre, op),
                Err(Refusal::of::<Freeze>(AuthorityError::ViaNotAllowed))
            );
        }
    }
    assert_eq!(
        transact_via(
            pda,
            Some(GOVERNANCE),
            &world(&governed()),
            freeze_account(STRANGER)
        ),
        Ok(one_frozen.clone())
    );
    assert_eq!(
        transact_via(
            pda,
            Some(GOVERNANCE),
            &one_frozen,
            release_account(STRANGER)
        ),
        Ok(with_marker(world(&governed()), STRANGER, &[]))
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
    let one_frozen = transact(STRANGER, &world(&replaced), freeze_account(AUTHORITY)).unwrap();
    assert_eq!(one_frozen, with_frozen(&replaced, &[AUTHORITY]));
    assert_eq!(
        transact(STRANGER, &one_frozen, release_account(AUTHORITY)),
        Ok(with_marker(world(&replaced), AUTHORITY, &[]))
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

#[test]
fn a_frozen_account_s_message_is_refused_at_the_config() {
    let appointed = config(holder(AUTHORITY));
    let both_frozen = with_frozen(&appointed, &[AUTHORITY, ADMIN]);
    assert_eq!(
        transact(ADMIN, &both_frozen, Message::AdminSet(1).into()),
        Err(e218())
    );
    assert_eq!(
        transact(AUTHORITY, &both_frozen, Message::AuthoritySet(1).into()),
        Err(e218())
    );
    let other_frozen = with_frozen(&appointed, &[STRANGER]);
    assert_eq!(
        transact(ADMIN, &other_frozen, Message::AdminSet(1).into()),
        Ok(with_frozen(&valued(appointed.clone(), 1), &[STRANGER]))
    );
    assert_eq!(
        transact(AUTHORITY, &other_frozen, Message::AuthoritySet(2).into()),
        Ok(with_frozen(&valued(appointed.clone(), 2), &[STRANGER]))
    );
}

#[test]
fn the_holder_s_check_and_the_program_s_freeze_come_before_a_frozen_account() {
    let stranger_frozen = with_frozen(&config(holder(AUTHORITY)), &[STRANGER]);
    assert_eq!(
        transact(STRANGER, &stranger_frozen, Message::AdminSet(1).into()),
        Err(Refusal::of::<Admin>(AuthorityError::NotHolder))
    );
    let all_frozen = with_frozen(&frozen(config(holder(AUTHORITY))), &[ADMIN]);
    assert_eq!(
        transact(ADMIN, &all_frozen, Message::AdminSet(1).into()),
        Err(e217())
    );
}

#[test]
fn a_message_sent_straight_to_the_config_has_no_caller_to_freeze() {
    let accounts = [
        AUTHORITY,
        STRANGER,
        ELSEWHERE,
        config_actor().account_id,
        ADMIN,
    ];
    let pre = with_frozen(&config(holder(AUTHORITY)), &accounts);
    assert_eq!(
        run(&pre, config_actor(), None, &[], Message::Set(3).into()),
        Ok(with_frozen(
            &valued(config(holder(AUTHORITY)), 3),
            &accounts
        ))
    );
}

#[test]
fn a_frozen_account_s_exempt_message_runs() {
    let appointed = config(holder(AUTHORITY));
    let admin_frozen = with_frozen(&appointed, &[ADMIN]);
    assert_eq!(
        transact(ADMIN, &admin_frozen, Message::AdminExemptSet(6).into()),
        Ok(with_frozen(&valued(appointed.clone(), 6), &[ADMIN]))
    );
    let stranger_frozen = with_frozen(&appointed, &[STRANGER]);
    let counted = elsewhere(
        actor(STRANGER),
        None,
        true,
        &stranger_frozen,
        Message::ExemptCount(1),
    )
    .unwrap();
    assert_eq!(counted.post_state.as_deref(), Some(&[1u8][..]));
    assert!(requests(&counted).is_empty());
}

#[test]
fn a_frozen_account_keeps_its_management_operations() {
    let both_frozen = |config: &Config| with_frozen(config, &[AUTHORITY, ADMIN]);
    let appointed = || config(holder(AUTHORITY));
    assert_eq!(
        transact(
            ADMIN,
            &both_frozen(&appointed()),
            admin_op(Op::Nominate { next: SUCCESSOR })
        ),
        Ok(both_frozen(&Config {
            admin: AuthoritySlot::Held {
                holder: Holder {
                    account: ADMIN,
                    via: None,
                },
                nominee: Some(SUCCESSOR),
            },
            ..appointed()
        }))
    );
    assert_eq!(
        transact(ADMIN, &both_frozen(&appointed()), renounce()),
        Ok(both_frozen(&config(AuthoritySlot::Unset)))
    );
    assert_eq!(
        transact(AUTHORITY, &both_frozen(&appointed()), freeze()),
        Ok(both_frozen(&frozen(appointed())))
    );
    assert_eq!(
        transact(
            AUTHORITY,
            &both_frozen(&appointed()),
            freeze_account(STRANGER)
        ),
        Ok(with_frozen(&appointed(), &[AUTHORITY, STRANGER, ADMIN]))
    );
    let released = transact(
        AUTHORITY,
        &both_frozen(&appointed()),
        release_account(AUTHORITY),
    )
    .unwrap();
    assert_eq!(
        released,
        with_marker(with_frozen(&appointed(), &[ADMIN]), AUTHORITY, &[])
    );
    assert_eq!(
        transact(AUTHORITY, &released, Message::AuthoritySet(1).into()),
        Ok(with_marker(
            with_frozen(&valued(appointed(), 1), &[ADMIN]),
            AUTHORITY,
            &[]
        ))
    );
    let offered = config(AuthoritySlot::Held {
        holder: Holder {
            account: AUTHORITY,
            via: None,
        },
        nominee: Some(SUCCESSOR),
    });
    for sender in [AUTHORITY, ADMIN] {
        assert_eq!(
            transact(sender, &both_frozen(&appointed()), nominate(SUCCESSOR)),
            Ok(both_frozen(&offered))
        );
    }
    let all_three = |freeze| with_frozen(&config(freeze), &[AUTHORITY, SUCCESSOR, ADMIN]);
    let offered = transact(
        AUTHORITY,
        &all_three(holder(AUTHORITY)),
        nominate(SUCCESSOR),
    )
    .unwrap();
    assert_eq!(
        transact(SUCCESSOR, &offered, accept()),
        Ok(all_three(holder(SUCCESSOR)))
    );
    assert_eq!(
        transact(AUTHORITY, &both_frozen(&frozen(appointed())), release()),
        Ok(both_frozen(&appointed()))
    );
}

#[test]
fn a_frozen_pda_is_refused_through_its_program_and_keeps_its_management_operations() {
    let vault = AccountId::for_public_pda(&MULTISIG, &PdaSeed::new([7; 32]));
    let vault_admin = |freeze| Config {
        admin: AuthoritySlot::held(Holder {
            account: vault,
            via: Some(MULTISIG),
        }),
        ..config(freeze)
    };
    let vault_frozen = with_frozen(&vault_admin(holder(AUTHORITY)), &[vault]);
    assert_eq!(
        transact_via(
            vault,
            Some(MULTISIG),
            &vault_frozen,
            Message::AdminSet(1).into()
        ),
        Err(e218())
    );
    assert_eq!(
        transact_via(vault, Some(MULTISIG), &vault_frozen, renounce()),
        Ok(with_frozen(&vault_admin(AuthoritySlot::Unset), &[vault]))
    );
}

/// `message` handled at `to`, sent by `from` and signed by `to`'s account when `authorized`,
/// with all it sends, a request for clearance answered at the config. The response at `to`.
fn elsewhere(
    to: Actor,
    from: Option<Actor>,
    authorized: bool,
    pre: &States,
    message: Message,
) -> Result<Response, Refusal> {
    let mut states = pre.clone();
    let signers = if authorized {
        vec![to.account_id]
    } else {
        Vec::new()
    };
    let message = borsh::to_vec(&Wire::Program(message)).unwrap();
    let response = execute(&mut states, to, from, &signers, message)?;
    assert_eq!(
        states.get(&config_actor()),
        pre.get(&config_actor()),
        "a request for clearance writes no config"
    );
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

/// The freeze's own messages in `response`, each as the account it names and its bytes.
fn marker_messages(response: &Response) -> Vec<(AccountId, Vec<u8>)> {
    response
        .sync_messages
        .iter()
        .filter_map(|sent| match borsh::from_slice(&sent.message).unwrap() {
            Wire::<Message>::Library(LibraryMessage::Own { role, key, message }) => {
                assert_eq!(role, "freeze");
                let account = AccountId::new(key);
                assert_eq!(sent.to, marker(account));
                // A marker that holds no state yet is reached only through its seed.
                let seeds: Vec<&PdaSeed> = sent.pda_seeds.iter().collect();
                let [seed] = seeds.as_slice() else {
                    panic!("one seed with each marker message");
                };
                assert_eq!(
                    AccountId::for_public_pda(&PROGRAM, seed),
                    sent.to.account_id
                );
                Some((account, message))
            }
            _ => None,
        })
        .collect()
}

#[test]
fn a_freeze_reaches_a_message_at_another_actor_whoever_sent_it() {
    let frozen_program = world(&frozen(config(holder(AUTHORITY))));
    let appointed = world(&config(holder(AUTHORITY)));
    for from in senders() {
        for authorized in [true, false] {
            let at_stranger =
                |pre: &States| elsewhere(actor(STRANGER), from, authorized, pre, Message::Count(1));
            assert_eq!(at_stranger(&frozen_program).err(), Some(e217()));
            let counted = at_stranger(&appointed).unwrap();
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
            actor(STRANGER),
            from,
            true,
            &world(&frozen(config(holder(AUTHORITY)))),
            Message::ExemptCount(1),
        )
        .unwrap();
        assert_eq!(counted.post_state.as_deref(), Some(&[1u8][..]));
        assert!(requests(&counted).is_empty());
    }
}

#[test]
fn a_freeze_reaches_a_message_the_config_sends_on() {
    let credit = |pre: &States| {
        elsewhere(
            actor(SUCCESSOR),
            Some(config_actor()),
            false,
            pre,
            Message::Credit(2),
        )
    };
    assert_eq!(
        credit(&world(&frozen(config(holder(AUTHORITY))))).err(),
        Some(e217())
    );
    assert_eq!(
        credit(&world(&config(holder(AUTHORITY))))
            .unwrap()
            .post_state
            .as_deref(),
        Some(&[2u8][..])
    );
}

#[test]
fn after_a_release_a_message_at_another_actor_runs_again() {
    let released = send(AUTHORITY, &frozen(config(holder(AUTHORITY))), release()).unwrap();
    let counted = elsewhere(
        actor(STRANGER),
        None,
        false,
        &world(&released),
        Message::Count(1),
    )
    .unwrap();
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

#[test]
fn the_config_asks_the_marker_of_a_caller_it_knows_and_no_other() {
    let check = borsh::to_vec(&MarkerMessage::Check).unwrap();
    let appointed = config(holder(AUTHORITY));
    let seat = at(actor(ADMIN), None, true, None, Message::AdminSet(1).into()).unwrap();
    let hop: Wire<Message> = borsh::from_slice(&seat.sync_messages[0].message).unwrap();
    let through_seat = at(
        config_actor(),
        Some(actor(ADMIN)),
        false,
        Some(&appointed),
        hop,
    )
    .unwrap();
    assert_eq!(through_seat.sync_messages.len(), 1);
    assert_eq!(marker_messages(&through_seat), [(ADMIN, check.clone())]);
    for (authorized, asked) in [(true, vec![(STRANGER, check.clone())]), (false, vec![])] {
        let request = Wire::<Message>::Library(LibraryMessage::Clearance(Clearance { authorized }));
        let answer = at(
            config_actor(),
            Some(actor(STRANGER)),
            false,
            Some(&appointed),
            request,
        )
        .unwrap();
        assert_eq!(answer.sync_messages.len(), asked.len());
        assert_eq!(marker_messages(&answer), asked);
    }
    let straight = at(
        config_actor(),
        None,
        false,
        Some(&appointed),
        Message::Set(1).into(),
    )
    .unwrap();
    assert!(straight.sync_messages.is_empty());
}

#[test]
fn a_freeze_operation_on_an_account_asks_its_marker_and_no_other() {
    let appointed = config(holder(AUTHORITY));
    for (op, message) in [
        (freeze_account(STRANGER), MarkerMessage::Mark),
        (release_account(STRANGER), MarkerMessage::Unmark),
    ] {
        let seat = at(actor(AUTHORITY), None, true, None, op).unwrap();
        let hop: Wire<Message> = borsh::from_slice(&seat.sync_messages[0].message).unwrap();
        let at_config = at(
            config_actor(),
            Some(actor(AUTHORITY)),
            false,
            Some(&appointed),
            hop,
        )
        .unwrap();
        assert_eq!(at_config.sync_messages.len(), 1);
        assert_eq!(
            marker_messages(&at_config),
            [(STRANGER, borsh::to_vec(&message).unwrap())]
        );
    }
}

#[test]
fn a_frozen_account_is_refused_at_its_actor_when_it_authorized_the_transaction() {
    let stranger_frozen = with_frozen(&config(holder(AUTHORITY)), &[STRANGER]);
    for from in senders() {
        assert_eq!(
            elsewhere(
                actor(STRANGER),
                from,
                true,
                &stranger_frozen,
                Message::Count(1)
            )
            .err(),
            Some(e218())
        );
        let counted = elsewhere(
            actor(STRANGER),
            from,
            false,
            &stranger_frozen,
            Message::Count(1),
        )
        .unwrap();
        assert_eq!(counted.post_state.as_deref(), Some(&[1u8][..]));
        assert_eq!(requests(&counted), [Clearance { authorized: false }]);
    }
}

#[test]
fn a_credit_lands_at_a_frozen_account_that_did_not_sign_the_transaction() {
    let successor_frozen = with_frozen(&config(holder(AUTHORITY)), &[SUCCESSOR]);
    let credit = |authorized| {
        elsewhere(
            actor(SUCCESSOR),
            Some(config_actor()),
            authorized,
            &successor_frozen,
            Message::Credit(2),
        )
    };
    assert_eq!(
        credit(false).unwrap().post_state.as_deref(),
        Some(&[2u8][..])
    );
    assert_eq!(credit(true).err(), Some(e218()));
}

#[test]
fn a_frozen_account_is_matched_by_its_whole_id() {
    let appointed = config(holder(AUTHORITY));
    let frozen_ids = [id(3, 1), id(3, 3)];
    let pre = with_frozen(&appointed, &frozen_ids);
    let counted = elsewhere(actor(id(3, 2)), None, true, &pre, Message::Count(1)).unwrap();
    assert_eq!(requests(&counted), [Clearance { authorized: true }]);
    assert_eq!(
        elsewhere(actor(id(3, 3)), None, true, &pre, Message::Count(1)).err(),
        Some(e218())
    );
    for (admin, refusal) in [(id(3, 2), None), (id(3, 3), Some(e218()))] {
        let pre = with_frozen(
            &Config {
                admin: holder(admin),
                ..appointed.clone()
            },
            &frozen_ids,
        );
        assert_eq!(
            transact(admin, &pre, Message::AdminSet(1).into()).err(),
            refusal
        );
    }
}

#[test]
fn a_frozen_sender_does_not_freeze_the_actor_it_sends_to() {
    let successor_frozen = with_frozen(&config(holder(AUTHORITY)), &[SUCCESSOR]);
    let counted = elsewhere(
        actor(STRANGER),
        Some(actor(SUCCESSOR)),
        true,
        &successor_frozen,
        Message::Count(1),
    )
    .unwrap();
    assert_eq!(counted.post_state.as_deref(), Some(&[1u8][..]));
    assert_eq!(requests(&counted), [Clearance { authorized: true }]);
}

#[test]
fn after_an_account_s_release_its_messages_run_again() {
    let appointed = config(holder(AUTHORITY));
    let stranger_frozen = with_frozen(&appointed, &[STRANGER]);
    let released = transact(AUTHORITY, &stranger_frozen, release_account(STRANGER)).unwrap();
    let counted = elsewhere(actor(STRANGER), None, true, &released, Message::Count(1)).unwrap();
    assert_eq!(counted.post_state.as_deref(), Some(&[1u8][..]));
    let admin_frozen = with_frozen(&appointed, &[ADMIN]);
    let released = transact(AUTHORITY, &admin_frozen, release_account(ADMIN)).unwrap();
    assert_eq!(
        transact(ADMIN, &released, Message::AdminSet(4).into()),
        Ok(with_marker(
            world(&valued(appointed.clone(), 4)),
            ADMIN,
            &[]
        ))
    );
}

#[test]
fn no_program_message_runs_at_a_frozen_account_s_marker() {
    for pre in [config(holder(AUTHORITY)), frozen(config(holder(AUTHORITY)))] {
        let pre = with_frozen(&pre, &[STRANGER]);
        for from in senders() {
            for authorized in [true, false] {
                for message in [
                    Message::Count(1),
                    Message::ExemptCount(1),
                    Message::Write(vec![1]),
                    Message::ExemptWrite(Vec::new()),
                ] {
                    assert_eq!(
                        elsewhere(marker(STRANGER), from, authorized, &pre, message).err(),
                        Some(e227())
                    );
                }
            }
        }
        assert_eq!(
            elsewhere(
                marker(STRANGER),
                Some(config_actor()),
                false,
                &pre,
                Message::Credit(1)
            )
            .err(),
            Some(e227())
        );
    }
}

#[test]
fn a_marker_that_holds_no_mark_runs_any_handler() {
    let appointed = config(holder(AUTHORITY));
    for state in [vec![Vec::new()], strays().to_vec()].concat() {
        let pre = with_marker(world(&appointed), STRANGER, &state);
        let credited = elsewhere(
            marker(STRANGER),
            Some(config_actor()),
            false,
            &pre,
            Message::Credit(2),
        )
        .unwrap();
        let count = state.first().copied().unwrap_or(0) + 2;
        assert_eq!(credited.post_state.as_deref(), Some(&[count][..]));
    }
}

#[test]
fn no_handler_writes_a_state_that_starts_with_the_mark() {
    let appointed = world(&config(holder(AUTHORITY)));
    for to in [actor(STRANGER), marker(STRANGER)] {
        for forged in [MARK.to_vec(), [&MARK[..], &[1]].concat()] {
            for message in [
                Message::Write(forged.clone()),
                Message::ExemptWrite(forged.clone()),
            ] {
                assert_eq!(
                    elsewhere(to, None, false, &appointed, message).err(),
                    Some(e228())
                );
            }
        }
        for state in [
            Vec::new(),
            MARK[..31].to_vec(),
            [&[0][..], &MARK[..]].concat(),
        ] {
            let written =
                elsewhere(to, None, false, &appointed, Message::Write(state.clone())).unwrap();
            assert_eq!(written.post_state.as_deref(), Some(&state[..]));
        }
    }
}

#[test]
fn a_marker_message_that_does_not_decode_is_refused() {
    for bytes in [Vec::new(), vec![3], vec![0, 0]] {
        let own = Wire::<Message>::Library(LibraryMessage::Own {
            role: "freeze".into(),
            key: *STRANGER.value(),
            message: bytes,
        });
        assert_eq!(
            at(marker(STRANGER), Some(config_actor()), false, None, own).err(),
            Some(Refusal::of::<Freeze>(AuthorityError::MessageDoesNotDecode))
        );
    }
}
