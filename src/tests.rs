use std::collections::BTreeSet;

use lee_core::account::ActorState;
use lee_core::program::{AsyncMessage, SyncMessage};

use super::*;

const KEY: AccountId = AccountId::new([1; 32]);
const OTHER: AccountId = AccountId::new([2; 32]);
const GOVERNANCE: AccountId = AccountId::new([6; 32]);
const PROGRAM: AccountId = AccountId::new([0xA0; 32]);
const ELSEWHERE: AccountId = AccountId::new([0xB0; 32]);

fn caller(account: AccountId, via: Option<AccountId>) -> Caller {
    Caller { account, via }
}

fn held(account: AccountId) -> AuthoritySlot {
    AuthoritySlot::held(Holder { account, via: None })
}

/// A role that answers to nobody, as the admin does.
struct Hold;

impl Role for Hold {
    const NAME: &'static str = "holder";
}

impl<C: HasSlot<Hold>> Policy<C> for Hold {}

struct One(AuthoritySlot);

impl HasSlot<Hold> for One {
    fn slot(&self) -> &AuthoritySlot {
        &self.0
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.0
    }
}

/// The slot `op` leaves.
fn run_with(
    caller: Option<Caller>,
    gate: InitGate,
    slot: AuthoritySlot,
    op: Op,
) -> Result<AuthoritySlot, AuthorityError> {
    let mut config = One(slot);
    let ctx = Ctx {
        caller,
        program: PROGRAM,
        gate,
    };
    run_op::<Hold, One>(&ctx, &mut config, op).map_err(|refusal| {
        assert_eq!(refusal.role, "holder");
        refusal.error
    })?;
    Ok(config.0)
}

fn run(
    caller: Option<Caller>,
    slot: AuthoritySlot,
    op: Op,
) -> Result<AuthoritySlot, AuthorityError> {
    run_with(caller, InitGate::Deployer, slot, op)
}

#[test]
fn codes_are_stable() {
    let expected: [(AuthorityError, u16); 20] = [
        (AuthorityError::NotInitialized, 101),
        (AuthorityError::Renounced, 102),
        (AuthorityError::NotHolder, 103),
        (AuthorityError::MissingSignature, 104),
        (AuthorityError::SeedProofFailed, 107),
        (AuthorityError::MessageDoesNotDecode, 108),
        (AuthorityError::ConfigDoesNotDecode, 109),
        (AuthorityError::NoNomination, 113),
        (AuthorityError::NotNominee, 114),
        (AuthorityError::NotGenesis, 115),
        (AuthorityError::NotDeployer, 117),
        (AuthorityError::AlreadyInstalled, 118),
        (AuthorityError::NotOwnProgram, 119),
        (AuthorityError::ViaNotAllowed, 120),
        (AuthorityError::ForwardedNotAtConfig, 121),
        (AuthorityError::NotThroughSeat, 122),
        (AuthorityError::NotFromConfig, 123),
        (AuthorityError::SlotChanged, 126),
        (AuthorityError::NotInstalled, 131),
        (AuthorityError::LibrarySend, 135),
    ];
    for (error, code) in &expected {
        assert_eq!(error.code(), *code, "{error:?}");
    }
    let mut codes: Vec<u16> = AuthorityError::ALL
        .iter()
        .map(AuthorityError::code)
        .collect();
    codes.sort_unstable();
    codes.dedup();
    assert_eq!(
        codes.len(),
        AuthorityError::ALL.len(),
        "two refusals share a code"
    );
    assert_eq!(
        AuthorityError::NotHolder.text("admin"),
        "lez-authority/admin E103: the signing account is not the admin"
    );
    assert_eq!(
        Refusal::of::<Hold>(AuthorityError::NotDeployer).text(),
        "lez-authority/holder E117: only the deployer installs the first holder: the program's own account, through its seat at top level"
    );
}

#[test]
fn a_key_holder_acts_at_top_level_only() {
    let slot = held(KEY);
    assert_eq!(slot.check(&caller(KEY, None)), Ok(()));
    assert_eq!(
        slot.check(&caller(KEY, Some(GOVERNANCE))),
        Err(AuthorityError::ViaNotAllowed)
    );
    assert_eq!(
        slot.check(&caller(OTHER, None)),
        Err(AuthorityError::NotHolder)
    );
}

#[test]
fn a_pda_holder_acts_through_its_program_only() {
    let slot = AuthoritySlot::held(Holder {
        account: KEY,
        via: Some(GOVERNANCE),
    });
    assert_eq!(slot.check(&caller(KEY, Some(GOVERNANCE))), Ok(()));
    assert_eq!(
        slot.check(&caller(KEY, None)),
        Err(AuthorityError::ViaNotAllowed)
    );
    assert_eq!(
        slot.check(&caller(KEY, Some(OTHER))),
        Err(AuthorityError::ViaNotAllowed)
    );
}

#[test]
fn unset_and_renounced_slots_refuse_everyone() {
    assert_eq!(
        AuthoritySlot::Unset.check(&caller(KEY, None)),
        Err(AuthorityError::NotInstalled)
    );
    assert_eq!(
        AuthoritySlot::Renounced.check(&caller(KEY, None)),
        Err(AuthorityError::Renounced)
    );
}

#[test]
fn the_deployer_installs_itself_and_the_slot_must_stay_that_way() {
    assert_eq!(
        run(Some(caller(PROGRAM, None)), AuthoritySlot::Unset, Op::Init),
        Ok(held(PROGRAM))
    );
}

#[test]
fn another_account_cannot_install() {
    assert_eq!(
        run(Some(caller(KEY, None)), AuthoritySlot::Unset, Op::Init),
        Err(AuthorityError::NotDeployer)
    );
}

#[test]
fn the_deployer_reached_through_a_program_cannot_install() {
    assert_eq!(
        run(
            Some(caller(PROGRAM, Some(GOVERNANCE))),
            AuthoritySlot::Unset,
            Op::Init
        ),
        Err(AuthorityError::NotDeployer)
    );
}

#[test]
fn with_a_genesis_key_only_that_key_installs() {
    assert_eq!(
        run_with(
            Some(caller(KEY, None)),
            InitGate::Genesis(KEY),
            AuthoritySlot::Unset,
            Op::Init
        ),
        Ok(held(KEY))
    );
}

#[test]
fn with_a_genesis_key_the_deployer_is_refused() {
    assert_eq!(
        run_with(
            Some(caller(PROGRAM, None)),
            InitGate::Genesis(KEY),
            AuthoritySlot::Unset,
            Op::Init
        ),
        Err(AuthorityError::NotGenesis)
    );
}

fn held_by_key_with(nominee: Option<AccountId>) -> AuthoritySlot {
    AuthoritySlot::Held {
        holder: Holder {
            account: KEY,
            via: None,
        },
        nominee,
    }
}

#[test]
fn nominate_records_replaces_and_cancels() {
    let by_key = || Some(caller(KEY, None));
    let slot = run(
        by_key(),
        held_by_key_with(None),
        Op::Nominate { next: OTHER },
    )
    .unwrap();
    assert_eq!(slot.nominee(), Some(OTHER));
    let slot = run(by_key(), slot, Op::Nominate { next: GOVERNANCE }).unwrap();
    assert_eq!(slot.nominee(), Some(GOVERNANCE));
    let slot = run(
        by_key(),
        slot,
        Op::Nominate {
            next: AccountId::default(),
        },
    )
    .unwrap();
    assert_eq!(slot, held(KEY));
}

#[test]
fn only_the_holder_nominates() {
    assert_eq!(
        run(
            Some(caller(OTHER, None)),
            held_by_key_with(None),
            Op::Nominate { next: OTHER }
        ),
        Err(AuthorityError::NotHolder)
    );
}

#[test]
fn a_key_nominee_accepts_at_top_level() {
    assert_eq!(
        run(
            Some(caller(OTHER, None)),
            held_by_key_with(Some(OTHER)),
            Op::Accept { pda_seed: None }
        ),
        Ok(held(OTHER))
    );
}

#[test]
fn a_pda_nominee_accepts_through_its_program_with_the_seed() {
    let seed = PdaSeed::new([9; 32]);
    let pda = AccountId::for_public_pda(&GOVERNANCE, &seed);
    assert_eq!(
        run(
            Some(caller(pda, Some(GOVERNANCE))),
            held_by_key_with(Some(pda)),
            Op::Accept {
                pda_seed: Some(seed)
            }
        ),
        Ok(AuthoritySlot::held(Holder {
            account: pda,
            via: Some(GOVERNANCE)
        }))
    );
}

#[test]
fn a_pda_nominee_with_the_wrong_seed_is_refused() {
    let pda = AccountId::for_public_pda(&GOVERNANCE, &PdaSeed::new([9; 32]));
    assert_eq!(
        run(
            Some(caller(pda, Some(GOVERNANCE))),
            held_by_key_with(Some(pda)),
            Op::Accept {
                pda_seed: Some(PdaSeed::new([8; 32]))
            }
        ),
        Err(AuthorityError::SeedProofFailed)
    );
}

#[test]
fn a_nominee_reached_through_a_program_without_a_seed_is_refused() {
    assert_eq!(
        run(
            Some(caller(OTHER, Some(GOVERNANCE))),
            held_by_key_with(Some(OTHER)),
            Op::Accept { pda_seed: None }
        ),
        Err(AuthorityError::SeedProofFailed)
    );
}

#[test]
fn only_the_nominee_accepts() {
    assert_eq!(
        run(
            Some(caller(GOVERNANCE, None)),
            held_by_key_with(Some(OTHER)),
            Op::Accept { pda_seed: None }
        ),
        Err(AuthorityError::NotNominee)
    );
}

#[test]
fn accept_without_a_nomination_is_refused() {
    assert_eq!(
        run(
            Some(caller(OTHER, None)),
            held_by_key_with(None),
            Op::Accept { pda_seed: None }
        ),
        Err(AuthorityError::NoNomination)
    );
}

#[test]
fn accept_after_renounce_is_refused() {
    let slot = run(
        Some(caller(KEY, None)),
        held_by_key_with(Some(OTHER)),
        Op::Renounce,
    );
    assert_eq!(slot, Ok(AuthoritySlot::Renounced));
    assert_eq!(
        run(
            Some(caller(OTHER, None)),
            AuthoritySlot::Renounced,
            Op::Accept { pda_seed: None }
        ),
        Err(AuthorityError::Renounced)
    );
}

#[test]
fn install_refuses_a_held_slot() {
    assert_eq!(
        run(Some(caller(PROGRAM, None)), held(KEY), Op::Init),
        Err(AuthorityError::AlreadyInstalled)
    );
}

#[test]
fn the_wire_keeps_the_program_s_encoding_and_marks_the_library_s() {
    let program: Wire<u16> = Wire::Program(7);
    assert_eq!(
        borsh::to_vec(&program).unwrap(),
        borsh::to_vec(&7u16).unwrap()
    );
    let op: Wire<u16> = Wire::op::<Hold>(Op::Init);
    let bytes = borsh::to_vec(&op).unwrap();
    assert_eq!(bytes[0], LIBRARY_TAG);
    assert_eq!(borsh::from_slice::<Wire<u16>>(&bytes).unwrap(), op);
    assert_eq!(
        borsh::from_slice::<Wire<u16>>(&borsh::to_vec(&7u16).unwrap()).unwrap(),
        program
    );
}

// A program with two roles. The second answers to the first, as a freeze authority would to
// the admin.

struct First;

impl Role for First {
    const NAME: &'static str = "first";
}

impl<C: HasSlot<First>> Policy<C> for First {}

struct Second;

impl Role for Second {
    const NAME: &'static str = "second";
}

impl<C: HasSlot<First> + HasSlot<Second>> Policy<C> for Second {
    fn may_install(ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        ctx.check::<First>(HasSlot::<First>::slot(config))
    }

    fn may_nominate(ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        ctx.check::<Second>(HasSlot::<Second>::slot(config))
            .or_else(|_| ctx.check::<First>(HasSlot::<First>::slot(config)))
    }

    fn renounced() -> AuthoritySlot {
        AuthoritySlot::Unset
    }
}

/// A role the program does not have.
struct Third;

impl Role for Third {
    const NAME: &'static str = "third";
}

#[derive(Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct Two {
    value: u64,
    first: AuthoritySlot,
    second: AuthoritySlot,
}

impl HasSlot<First> for Two {
    fn slot(&self) -> &AuthoritySlot {
        &self.first
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.first
    }
}

impl HasSlot<Second> for Two {
    fn slot(&self) -> &AuthoritySlot {
        &self.second
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.second
    }
}

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
enum Msg {
    SetValue(u64),
    GatedValue(u64),
    ClearSecond,
    ToTarget(AccountId),
    ToElsewhere(AccountId),
    Mark,
    Pass { to: AccountId, bytes: Vec<u8> },
    PassAsync { to: AccountId, bytes: Vec<u8> },
}

impl Gated for Msg {
    fn needs<R: Role>(&self) -> bool {
        matches!(
            self,
            Msg::GatedValue(_) | Msg::ToTarget(_) | Msg::ToElsewhere(_)
        ) && std::any::TypeId::of::<R>() == std::any::TypeId::of::<Second>()
    }

    fn only_from_config(&self) -> bool {
        matches!(self, Msg::Mark)
    }
}

/// A plain LEZ handler: the config where it runs, its own state anywhere else.
fn handle(input: &ReceiveInput, message: Msg) -> Response {
    let pre: &[u8] = &input.pre_state;
    if let Msg::Mark = message {
        return Response::set_state(vec![1]);
    }
    if let Msg::Pass { to, bytes } = message {
        let mut response = Response::keep_state();
        response.sync_messages.push(SyncMessage {
            to: Actor::new(to, PROGRAM),
            message: bytes,
            pda_seeds: BTreeSet::new(),
        });
        return response;
    }
    if let Msg::PassAsync { to, bytes } = message {
        let mut response = Response::keep_state();
        response.async_messages.push(AsyncMessage {
            to: Actor::new(to, PROGRAM),
            message: bytes,
        });
        return response;
    }
    let mut config: Two = if pre.is_empty() {
        Two::default()
    } else {
        borsh::from_slice(pre).unwrap()
    };
    let mut send = None;
    match message {
        Msg::SetValue(value) | Msg::GatedValue(value) => config.value = value,
        Msg::ClearSecond => config.second = AuthoritySlot::Unset,
        Msg::ToTarget(target) => send = Some(Actor::new(target, PROGRAM)),
        Msg::ToElsewhere(target) => send = Some(Actor::new(target, ELSEWHERE)),
        Msg::Mark | Msg::Pass { .. } | Msg::PassAsync { .. } => unreachable!(),
    }
    let response = Response::set_state(borsh::to_vec(&config).unwrap());
    match send {
        Some(to) => response.send_sync(to, &Msg::Mark),
        None => response,
    }
}

const SEED: PdaSeed = seed(b"two-roles/v1");

fn seat_of(sender: AccountId) -> Actor {
    Actor::new(sender, PROGRAM)
}

fn config_actor() -> Actor {
    Actor::new(config_account(PROGRAM, &SEED), PROGRAM)
}

fn at(
    receiver: Actor,
    from: Option<Actor>,
    authorized: bool,
    pre: Option<&Two>,
    message: Wire<Msg>,
) -> Result<Response, Refusal> {
    let input = ReceiveInput {
        receiver,
        from,
        is_authorized: authorized,
        pre_state: ActorState::from(pre.map(|c| borsh::to_vec(c).unwrap()).unwrap_or_default()),
        message: borsh::to_vec(&message).unwrap(),
    };
    try_guard::<(First, Second), Two, Msg>(&input, &SEED, InitGate::Deployer, message, handle)
}

/// From `sender`'s seat to the config. What the config answered.
fn through_seat(
    sender: AccountId,
    pre: Option<&Two>,
    message: Wire<Msg>,
) -> Result<Response, Refusal> {
    let hop = at(seat_of(sender), None, true, None, message)?;
    assert!(hop.post_state.is_none(), "the seat only forwards");
    let hop: Wire<Msg> = borsh::from_slice(&hop.sync_messages[0].message).unwrap();
    at(config_actor(), Some(seat_of(sender)), false, pre, hop)
}

/// The config as written back.
fn send(sender: AccountId, pre: Option<&Two>, message: Wire<Msg>) -> Result<Two, Refusal> {
    let response = through_seat(sender, pre, message)?;
    Ok(borsh::from_slice(response.post_state.as_ref().unwrap()).unwrap())
}

/// The first role held by the deployer, the second held by `second` if any.
fn two(second: Option<AccountId>) -> Two {
    Two {
        value: 0,
        first: held(PROGRAM),
        second: second.map_or(AuthoritySlot::Unset, held),
    }
}

#[test]
fn only_the_first_role_creates_the_config() {
    assert_eq!(
        send(PROGRAM, None, Wire::op::<Second>(Op::Init)),
        Err(Refusal::of::<First>(AuthorityError::NotInitialized))
    );
    assert_eq!(
        send(PROGRAM, None, Wire::op::<First>(Op::Init)),
        Ok(two(None))
    );
}

#[test]
fn the_first_role_installs_and_nominates_for_the_second() {
    assert_eq!(
        send(KEY, Some(&two(None)), Wire::op::<Second>(Op::Init)),
        Err(Refusal::of::<First>(AuthorityError::NotHolder))
    );
    let config = send(PROGRAM, Some(&two(None)), Wire::op::<Second>(Op::Init)).unwrap();
    assert_eq!(config, two(Some(PROGRAM)));
    let nominate = Wire::op::<Second>(Op::Nominate { next: KEY });
    let config = send(PROGRAM, Some(&config), nominate).unwrap();
    let accept = Wire::op::<Second>(Op::Accept { pda_seed: None });
    let config = send(KEY, Some(&config), accept).unwrap();
    assert_eq!(config, two(Some(KEY)));
    let nominate = Wire::op::<Second>(Op::Nominate { next: OTHER });
    let config = send(PROGRAM, Some(&config), nominate).unwrap();
    assert_eq!(config.second.nominee(), Some(OTHER));
    assert_eq!(
        send(
            OTHER,
            Some(&config),
            Wire::op::<Second>(Op::Nominate { next: OTHER })
        ),
        Err(Refusal::of::<First>(AuthorityError::NotHolder))
    );
}

#[test]
fn a_gated_message_takes_the_holder_of_its_role() {
    let config = send(KEY, Some(&two(Some(KEY))), Msg::GatedValue(5).into()).unwrap();
    assert_eq!(config.value, 5);
}

#[test]
fn the_first_roles_holder_does_not_hold_the_second() {
    assert_eq!(
        send(PROGRAM, Some(&two(Some(KEY))), Msg::GatedValue(5).into()),
        Err(Refusal::of::<Second>(AuthorityError::NotHolder))
    );
}

#[test]
fn require_refuses_a_message_that_skipped_the_seat() {
    let refused = at(
        config_actor(),
        None,
        false,
        Some(&two(Some(KEY))),
        Msg::GatedValue(3).into(),
    )
    .err();
    assert_eq!(
        refused,
        Some(Refusal::of::<Second>(AuthorityError::NotThroughSeat))
    );
}

#[test]
fn renouncing_the_second_role_leaves_it_for_the_first_to_fill() {
    let config = send(KEY, Some(&two(Some(KEY))), Wire::op::<Second>(Op::Renounce)).unwrap();
    assert_eq!(config, two(None));
    let config = send(PROGRAM, Some(&config), Wire::op::<Second>(Op::Init)).unwrap();
    assert_eq!(config, two(Some(PROGRAM)));
}

#[test]
fn the_first_role_stays_renounced_and_takes_the_second_with_it() {
    let config = send(PROGRAM, Some(&two(None)), Wire::op::<First>(Op::Renounce)).unwrap();
    assert_eq!(config.first, AuthoritySlot::Renounced);
    assert_eq!(
        send(PROGRAM, Some(&config), Wire::op::<First>(Op::Init)),
        Err(Refusal::of::<First>(AuthorityError::Renounced))
    );
    assert_eq!(
        send(PROGRAM, Some(&config), Wire::op::<Second>(Op::Init)),
        Err(Refusal::of::<First>(AuthorityError::Renounced))
    );
}

#[test]
fn a_handler_that_changes_the_second_slot_is_refused_by_its_name() {
    let refused = at(
        config_actor(),
        None,
        false,
        Some(&two(Some(KEY))),
        Msg::ClearSecond.into(),
    )
    .err();
    assert_eq!(
        refused,
        Some(Refusal::of::<Second>(AuthorityError::SlotChanged))
    );
}

#[test]
fn an_open_message_runs_where_it_is_sent() {
    let at_config = at(
        config_actor(),
        None,
        false,
        Some(&two(None)),
        Msg::SetValue(4).into(),
    )
    .ok()
    .unwrap();
    let config: Two = borsh::from_slice(at_config.post_state.as_ref().unwrap()).unwrap();
    assert_eq!(config.value, 4);
    let elsewhere = at(seat_of(KEY), None, false, None, Msg::SetValue(6).into())
        .ok()
        .unwrap();
    assert!(elsewhere.sync_messages.is_empty(), "nothing was forwarded");
    let state: Two = borsh::from_slice(elsewhere.post_state.as_ref().unwrap()).unwrap();
    assert_eq!(state.value, 6);
}

#[test]
fn before_init_even_an_open_message_at_the_config_is_refused() {
    assert_eq!(
        at(config_actor(), None, false, None, Msg::SetValue(4).into()).err(),
        Some(Refusal::of::<First>(AuthorityError::NotInitialized))
    );
}

#[test]
fn the_config_reaches_another_actor_of_the_program() {
    let target = AccountId::new([7; 32]);
    let response = through_seat(KEY, Some(&two(Some(KEY))), Msg::ToTarget(target).into())
        .ok()
        .unwrap();
    let [sent] = response.sync_messages.as_slice() else {
        panic!("one message to the target");
    };
    assert_eq!(sent.to, Actor::new(target, PROGRAM));
    let mark: Wire<Msg> = borsh::from_slice(&sent.message).unwrap();
    let at_target = at(sent.to, Some(config_actor()), false, None, mark)
        .ok()
        .unwrap();
    assert_eq!(at_target.post_state.as_deref(), Some(&[1u8][..]));
}

#[test]
fn a_config_only_message_from_anyone_else_is_refused() {
    let target = Actor::new(AccountId::new([7; 32]), PROGRAM);
    let refused = Some(Refusal::of::<First>(AuthorityError::NotFromConfig));
    assert_eq!(
        at(target, None, true, None, Msg::Mark.into()).err(),
        refused
    );
    assert_eq!(
        at(target, Some(seat_of(KEY)), true, None, Msg::Mark.into()).err(),
        refused
    );
    assert_eq!(
        at(
            config_actor(),
            Some(config_actor()),
            false,
            Some(&two(None)),
            Msg::Mark.into()
        )
        .err(),
        refused
    );
}

#[test]
fn a_vouched_message_may_call_another_program() {
    let target = AccountId::new([7; 32]);
    let response = through_seat(KEY, Some(&two(Some(KEY))), Msg::ToElsewhere(target).into())
        .ok()
        .unwrap();
    let [sent] = response.sync_messages.as_slice() else {
        panic!("one message to the other program");
    };
    assert_eq!(sent.to, Actor::new(target, ELSEWHERE));
}

#[test]
fn an_operation_sent_straight_to_the_config_is_refused() {
    let nominate = Wire::op::<First>(Op::Nominate { next: KEY });
    assert_eq!(
        at(config_actor(), None, false, Some(&two(None)), nominate).err(),
        Some(Refusal::of::<First>(AuthorityError::NotThroughSeat))
    );
}

#[test]
fn an_unsigned_sender_is_refused_at_its_seat() {
    assert_eq!(
        at(seat_of(KEY), None, false, None, Wire::op::<First>(Op::Init)).err(),
        Some(Refusal::of::<First>(AuthorityError::MissingSignature))
    );
}

#[test]
fn a_forward_from_another_program_is_refused() {
    let hop = Wire::<Msg>::Library(LibraryMessage::Forwarded(Forwarded {
        via: None,
        inner: borsh::to_vec(&Wire::<Msg>::op::<First>(Op::Init)).unwrap(),
    }));
    let foreign = Actor::new(KEY, ELSEWHERE);
    assert_eq!(
        at(config_actor(), Some(foreign), false, None, hop.clone()).err(),
        Some(Refusal::of::<First>(AuthorityError::NotOwnProgram))
    );
    assert_eq!(
        at(seat_of(KEY), Some(seat_of(OTHER)), true, None, hop).err(),
        Some(Refusal::of::<First>(AuthorityError::ForwardedNotAtConfig))
    );
}

#[test]
fn a_handler_cannot_forge_a_hop() {
    let hop = Wire::<Msg>::Library(LibraryMessage::Forwarded(Forwarded {
        via: None,
        inner: borsh::to_vec(&Wire::<Msg>::op::<First>(Op::Nominate { next: OTHER })).unwrap(),
    }));
    let pass = Msg::Pass {
        to: config_actor().account_id,
        bytes: borsh::to_vec(&hop).unwrap(),
    };
    assert_eq!(
        at(seat_of(PROGRAM), None, false, None, pass.into()).err(),
        Some(Refusal::of::<First>(AuthorityError::LibrarySend))
    );
}

#[test]
fn a_handler_cannot_forge_a_hop_through_an_async_message() {
    let hop = Wire::<Msg>::Library(LibraryMessage::Forwarded(Forwarded {
        via: None,
        inner: borsh::to_vec(&Wire::<Msg>::op::<First>(Op::Nominate { next: OTHER })).unwrap(),
    }));
    let pass = Msg::PassAsync {
        to: config_actor().account_id,
        bytes: borsh::to_vec(&hop).unwrap(),
    };
    assert_eq!(
        at(seat_of(PROGRAM), None, false, None, pass.into()).err(),
        Some(Refusal::of::<First>(AuthorityError::LibrarySend))
    );
}

#[test]
fn a_handler_at_the_config_cannot_send_it_a_library_message() {
    let pass = Msg::Pass {
        to: config_actor().account_id,
        bytes: borsh::to_vec(&Wire::<Msg>::op::<First>(Op::Renounce)).unwrap(),
    };
    assert_eq!(
        at(config_actor(), None, false, Some(&two(None)), pass.into()).err(),
        Some(Refusal::of::<First>(AuthorityError::LibrarySend))
    );
}

#[test]
fn an_operation_for_a_role_the_program_lacks_does_not_decode() {
    assert_eq!(
        send(PROGRAM, Some(&two(None)), Wire::op::<Third>(Op::Init)),
        Err(Refusal::of::<First>(AuthorityError::MessageDoesNotDecode))
    );
}
