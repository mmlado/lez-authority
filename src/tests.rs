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
    type Slot = AuthoritySlot;
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
    let expected: [(AuthorityError, u16); 21] = [
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
        (AuthorityError::ByNominationOnly, 137),
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
        AuthorityError::NotHolder.text("admin", "admin"),
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
fn the_default_policy_makes_no_offer_on_a_slot_with_no_holder() {
    assert_eq!(
        run(
            Some(caller(PROGRAM, None)),
            AuthoritySlot::Unset,
            Op::Nominate { next: KEY }
        ),
        Err(AuthorityError::NotInstalled)
    );
}

#[test]
fn a_slot_with_an_offer_has_a_nominee_and_no_holder() {
    let offered = AuthoritySlot::Offered { nominee: KEY };
    assert_eq!(offered.holder(), None);
    assert_eq!(offered.nominee(), Some(KEY));
    assert_eq!(
        offered.check(&caller(KEY, None)),
        Err(AuthorityError::NotInstalled)
    );
}

#[test]
fn install_keeps_a_standing_offer() {
    assert_eq!(
        run(
            Some(caller(PROGRAM, None)),
            AuthoritySlot::Offered { nominee: KEY },
            Op::Init
        ),
        Ok(AuthoritySlot::Held {
            holder: Holder {
                account: PROGRAM,
                via: None,
            },
            nominee: Some(KEY),
        })
    );
}

#[test]
fn the_slot_s_existing_encodings_keep_their_bytes() {
    let key = borsh::to_vec(&KEY).unwrap();
    assert_eq!(borsh::to_vec(&AuthoritySlot::Unset).unwrap(), [0]);
    assert_eq!(
        borsh::to_vec(&held(KEY)).unwrap(),
        [&[1][..], &key, &[0, 0]].concat()
    );
    assert_eq!(borsh::to_vec(&AuthoritySlot::Renounced).unwrap(), [2]);
    assert_eq!(
        borsh::to_vec(&AuthoritySlot::Offered { nominee: KEY }).unwrap(),
        [&[3][..], &key].concat()
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
    const PROSE_NAME: &'static str = "first role";
    type Slot = AuthoritySlot;
}

impl<C: HasSlot<First>> Policy<C> for First {}

struct Second;

impl Role for Second {
    const NAME: &'static str = "second";
    type Slot = AuthoritySlot;
}

impl<C: HasSlot<First> + HasSlot<Second>> Policy<C> for Second {
    fn may_install(ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        ctx.check::<First>(HasSlot::<First>::slot(config))
    }

    fn may_nominate(ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        ctx.check::<Second>(HasSlot::<Second>::slot(config))
            .or_else(|_| ctx.check::<First>(HasSlot::<First>::slot(config)))
    }

    fn may_renounce(ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        Self::may_nominate(ctx, config)
    }

    fn renounced() -> AuthoritySlot {
        AuthoritySlot::Unset
    }
}

/// A role the program does not have.
struct Third;

impl Role for Third {
    const NAME: &'static str = "third";
    type Slot = AuthoritySlot;
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
fn an_offer_on_a_slot_with_no_holder_is_made_replaced_and_cancelled() {
    let offer = |next| Wire::op::<Second>(Op::Nominate { next });
    let config = send(PROGRAM, Some(&two(None)), offer(KEY)).unwrap();
    assert_eq!(config.second, AuthoritySlot::Offered { nominee: KEY });
    let config = send(PROGRAM, Some(&config), offer(OTHER)).unwrap();
    assert_eq!(config.second, AuthoritySlot::Offered { nominee: OTHER });
    let config = send(PROGRAM, Some(&config), offer(AccountId::default())).unwrap();
    assert_eq!(config, two(None));
    let config = send(PROGRAM, Some(&config), offer(AccountId::default())).unwrap();
    assert_eq!(config, two(None));
}

#[test]
fn only_the_offered_account_accepts_and_then_holds_the_role() {
    let offer = Wire::op::<Second>(Op::Nominate { next: KEY });
    let config = send(PROGRAM, Some(&two(None)), offer).unwrap();
    let accept = || Wire::op::<Second>(Op::Accept { pda_seed: None });
    assert_eq!(
        send(OTHER, Some(&config), accept()),
        Err(Refusal::of::<Second>(AuthorityError::NotNominee))
    );
    assert_eq!(send(KEY, Some(&config), accept()), Ok(two(Some(KEY))));
}

#[test]
fn an_accept_with_nothing_offered_is_refused_as_no_nomination() {
    let accept = || Wire::op::<Second>(Op::Accept { pda_seed: None });
    assert_eq!(
        send(KEY, Some(&two(None)), accept()),
        Err(Refusal::of::<Second>(AuthorityError::NoNomination))
    );
    let offer = |next| Wire::op::<Second>(Op::Nominate { next });
    let offered = send(PROGRAM, Some(&two(None)), offer(KEY)).unwrap();
    let cancelled = send(PROGRAM, Some(&offered), offer(AccountId::default())).unwrap();
    assert_eq!(
        send(KEY, Some(&cancelled), accept()),
        Err(Refusal::of::<Second>(AuthorityError::NoNomination))
    );
}

#[test]
fn a_nomination_does_not_revive_a_renounced_slot() {
    let renounced = Two {
        second: AuthoritySlot::Renounced,
        ..two(None)
    };
    assert_eq!(
        send(
            PROGRAM,
            Some(&renounced),
            Wire::op::<Second>(Op::Nominate { next: KEY })
        ),
        Err(Refusal::of::<Second>(AuthorityError::Renounced))
    );
}

#[test]
fn a_renounce_drops_a_pending_nominee() {
    let nominate = Wire::op::<Second>(Op::Nominate { next: OTHER });
    let pending = send(KEY, Some(&two(Some(KEY))), nominate).unwrap();
    let config = send(PROGRAM, Some(&pending), Wire::op::<Second>(Op::Renounce)).unwrap();
    assert_eq!(config, two(None));
    assert_eq!(
        send(
            OTHER,
            Some(&config),
            Wire::op::<Second>(Op::Accept { pda_seed: None })
        ),
        Err(Refusal::of::<Second>(AuthorityError::NoNomination))
    );
}

#[test]
fn a_renounce_withdraws_a_standing_offer() {
    let offer = Wire::op::<Second>(Op::Nominate { next: KEY });
    let offered = send(PROGRAM, Some(&two(None)), offer).unwrap();
    let config = send(PROGRAM, Some(&offered), Wire::op::<Second>(Op::Renounce)).unwrap();
    assert_eq!(config, two(None));
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

// A role that keeps more than its holder in its slot, as a freeze authority keeps what it
// froze.

struct Marking;

impl Role for Marking {
    const NAME: &'static str = "marking";
    type Slot = Marked;
}

/// What the marking role answers to a program message while its mark is zero.
const HALTED: AuthorityError = AuthorityError::Role {
    krate: "lez-marking-authority",
    code: 201,
    sentence: "the mark is zero",
};

/// What the marking role answers to a program message `OTHER` sent through its seat.
const BARRED: AuthorityError = AuthorityError::Role {
    krate: "lez-marking-authority",
    code: 202,
    sentence: "the sender is barred",
};

// Its own operation sets the mark, and a zero mark stops program messages, as a freeze does.
impl<C: HasSlot<Marking>> Policy<C> for Marking {
    const REFUSES_MESSAGES: bool = true;

    fn operate(ctx: &Ctx, config: &C, op: &[u8]) -> Result<Marked, Refusal> {
        let mark = borsh::from_slice(op)
            .map_err(|_| Refusal::of::<Marking>(AuthorityError::MessageDoesNotDecode))?;
        let slot = HasSlot::<Marking>::slot(config);
        ctx.check::<Marking>(slot.authority_slot())?;
        Ok(Marked {
            authority_slot: slot.authority_slot.clone(),
            mark,
        })
    }

    fn may_run(ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        if HasSlot::<Marking>::slot(config).mark == 0 {
            return Err(Refusal::of::<Marking>(HALTED));
        }
        if ctx.caller().is_some_and(|caller| caller.account() == OTHER) {
            return Err(Refusal::of::<Marking>(BARRED));
        }
        Ok(())
    }
}

#[derive(Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct Marked {
    authority_slot: AuthoritySlot,
    mark: u64,
}

impl Slot for Marked {
    fn authority_slot(&self) -> &AuthoritySlot {
        &self.authority_slot
    }
    fn authority_slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.authority_slot
    }
}

#[derive(Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct WithMark(Marked);

impl HasSlot<Marking> for WithMark {
    fn slot(&self) -> &Marked {
        &self.0
    }
    fn slot_mut(&mut self) -> &mut Marked {
        &mut self.0
    }
}

#[derive(BorshSerialize, BorshDeserialize)]
struct SetMark(u64);

impl Gated for SetMark {}

fn marked(authority_slot: AuthoritySlot) -> WithMark {
    WithMark(Marked {
        authority_slot,
        mark: 7,
    })
}

fn mark_op(config: &mut WithMark, account: AccountId, op: Op) {
    let ctx = Ctx {
        caller: Some(caller(account, None)),
        program: PROGRAM,
        gate: InitGate::Deployer,
    };
    run_op::<Marking, WithMark>(&ctx, config, op).unwrap();
}

#[test]
fn operations_change_only_the_holder_part_of_a_role_s_own_slot() {
    let mut config = marked(AuthoritySlot::Unset);
    mark_op(&mut config, PROGRAM, Op::Init);
    mark_op(&mut config, PROGRAM, Op::Nominate { next: KEY });
    mark_op(&mut config, KEY, Op::Accept { pda_seed: None });
    assert_eq!(config, marked(held(KEY)));
    mark_op(&mut config, KEY, Op::Renounce);
    assert_eq!(config, marked(AuthoritySlot::Renounced));
}

/// Runs `handler` at the config, with `pre` as the config's state.
fn at_config_with<Rs, C>(
    pre: &C,
    mark: u64,
    handler: fn(&ReceiveInput, SetMark) -> Response,
) -> Option<Refusal>
where
    Rs: Roles<C, SetMark>,
    C: BorshSerialize + BorshDeserialize + Default,
{
    let message = Wire::Program(SetMark(mark));
    let input = ReceiveInput {
        receiver: config_actor(),
        from: None,
        is_authorized: false,
        pre_state: ActorState::from(borsh::to_vec(pre).unwrap()),
        message: borsh::to_vec(&message).unwrap(),
    };
    try_guard::<Rs, C, SetMark>(&input, &SEED, InitGate::Deployer, message, handler).err()
}

/// Writes the mark, a part of the role's slot that the handler must leave alone.
fn set_mark(input: &ReceiveInput, message: SetMark) -> Response {
    let mut config: WithMark = borsh::from_slice(&input.pre_state).unwrap();
    config.0.mark = message.0;
    Response::set_state(borsh::to_vec(&config).unwrap())
}

#[test]
fn a_handler_that_changes_the_rest_of_a_role_s_own_slot_is_refused() {
    let pre = marked(held(PROGRAM));
    assert_eq!(at_config_with::<(Marking,), _>(&pre, 7, set_mark), None);
    assert_eq!(
        at_config_with::<(Marking,), _>(&pre, 8, set_mark),
        Some(Refusal::of::<Marking>(AuthorityError::SlotChanged))
    );
}

/// The freeze's shape: a root role that keeps only its holder, and a second that keeps more.
#[derive(Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct RootAndMark {
    first: AuthoritySlot,
    marking: Marked,
}

impl HasSlot<First> for RootAndMark {
    fn slot(&self) -> &AuthoritySlot {
        &self.first
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.first
    }
}

impl HasSlot<Marking> for RootAndMark {
    fn slot(&self) -> &Marked {
        &self.marking
    }
    fn slot_mut(&mut self) -> &mut Marked {
        &mut self.marking
    }
}

fn set_second_mark(input: &ReceiveInput, message: SetMark) -> Response {
    let mut config: RootAndMark = borsh::from_slice(&input.pre_state).unwrap();
    config.marking.mark = message.0;
    Response::set_state(borsh::to_vec(&config).unwrap())
}

#[test]
fn a_handler_that_changes_the_rest_of_the_second_role_s_slot_is_refused_by_its_name() {
    let pre = RootAndMark {
        first: held(PROGRAM),
        marking: Marked {
            authority_slot: held(KEY),
            mark: 7,
        },
    };
    assert_eq!(
        at_config_with::<(First, Marking), _>(&pre, 7, set_second_mark),
        None
    );
    assert_eq!(
        at_config_with::<(First, Marking), _>(&pre, 8, set_second_mark),
        Some(Refusal::of::<Marking>(AuthorityError::SlotChanged))
    );
}

fn replace_first(input: &ReceiveInput, _message: SetMark) -> Response {
    let mut config: RootAndMark = borsh::from_slice(&input.pre_state).unwrap();
    config.first = held(OTHER);
    Response::set_state(borsh::to_vec(&config).unwrap())
}

#[test]
fn a_handler_that_replaces_the_first_role_s_holder_is_refused_by_its_name() {
    let pre = RootAndMark {
        first: held(PROGRAM),
        marking: Marked {
            authority_slot: held(KEY),
            mark: 7,
        },
    };
    assert_eq!(
        at_config_with::<(First, Marking), _>(&pre, 7, replace_first),
        Some(Refusal::of::<First>(AuthorityError::SlotChanged))
    );
}

// A role whose holder is only ever appointed by nomination, named in prose as the freeze
// authority is.

struct Deputy;

impl Role for Deputy {
    const NAME: &'static str = "deputy";
    const PROSE_NAME: &'static str = "deputy authority";
    type Slot = AuthoritySlot;
}

impl<C: HasSlot<Deputy>> Policy<C> for Deputy {
    const BY_NOMINATION_ONLY: bool = true;
}

#[derive(Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct FirstAndDeputy {
    first: AuthoritySlot,
    deputy: AuthoritySlot,
}

impl HasSlot<First> for FirstAndDeputy {
    fn slot(&self) -> &AuthoritySlot {
        &self.first
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.first
    }
}

impl HasSlot<Deputy> for FirstAndDeputy {
    fn slot(&self) -> &AuthoritySlot {
        &self.deputy
    }
    fn slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.deputy
    }
}

#[test]
fn a_refusal_names_the_role_by_name_in_its_prefix_and_in_prose_in_its_sentence() {
    assert_eq!(
        Refusal::of::<Deputy>(AuthorityError::NotHolder).text(),
        "lez-authority/deputy E103: the signing account is not the deputy authority"
    );
    assert_eq!(
        Refusal::of::<Deputy>(AuthorityError::ByNominationOnly).text(),
        "lez-authority/deputy E137: the deputy authority is appointed by nomination, never installed"
    );
}

#[test]
fn a_role_s_own_refusal_prints_with_its_crate_s_prefix() {
    let own = AuthorityError::Role {
        krate: "lez-deputy-authority",
        code: 208,
        sentence: "the sender is neither the deputy authority nor the first",
    };
    assert_eq!(own.code(), 208);
    assert_eq!(
        Refusal::of::<Deputy>(own).text(),
        "lez-deputy-authority/deputy E208: the sender is neither the deputy authority nor the first"
    );
}

#[test]
fn a_role_appointed_only_by_nomination_refuses_every_init() {
    for deputy in [
        AuthoritySlot::Unset,
        AuthoritySlot::Offered { nominee: KEY },
        held(OTHER),
        AuthoritySlot::Renounced,
    ] {
        for who in [
            caller(PROGRAM, None),
            caller(KEY, None),
            caller(PROGRAM, Some(ELSEWHERE)),
        ] {
            let ctx = Ctx {
                caller: Some(who),
                program: PROGRAM,
                gate: InitGate::Deployer,
            };
            let before = || FirstAndDeputy {
                first: held(PROGRAM),
                deputy: deputy.clone(),
            };
            let mut config = before();
            assert_eq!(
                run_op::<Deputy, FirstAndDeputy>(&ctx, &mut config, Op::Init),
                Err(Refusal::of::<Deputy>(AuthorityError::ByNominationOnly))
            );
            assert_eq!(config, before());
        }
    }
}

fn keep<M>(_input: &ReceiveInput, _message: M) -> Response {
    Response::keep_state()
}

#[test]
fn a_role_appointed_only_by_nomination_may_be_the_second_role() {
    let pre = FirstAndDeputy {
        first: held(PROGRAM),
        deputy: AuthoritySlot::Unset,
    };
    assert_eq!(at_config_with::<(First, Deputy), _>(&pre, 0, keep), None);
}

#[test]
fn a_one_role_guard_names_its_root_in_prose() {
    let op = Wire::<SetMark>::op::<First>(Op::Renounce);
    let input = ReceiveInput {
        receiver: config_actor(),
        from: None,
        is_authorized: false,
        pre_state: ActorState::from(Vec::new()),
        message: borsh::to_vec(&op).unwrap(),
    };
    let refused =
        try_guard::<(First,), RootAndMark, SetMark>(&input, &SEED, InitGate::Deployer, op, keep)
            .err()
            .unwrap();
    assert_eq!(
        refused,
        Refusal::of::<First>(AuthorityError::NotThroughSeat)
    );
    assert_eq!(
        refused.text(),
        "lez-authority/first E122: a message that needs the first role must go through the sender's own actor"
    );
}

#[test]
fn an_operation_of_the_second_role_sent_straight_to_the_config_names_that_role() {
    let op = Wire::<SetMark>::op::<Deputy>(Op::Nominate { next: KEY });
    let input = ReceiveInput {
        receiver: config_actor(),
        from: None,
        is_authorized: false,
        pre_state: ActorState::from(borsh::to_vec(&FirstAndDeputy::default()).unwrap()),
        message: borsh::to_vec(&op).unwrap(),
    };
    let refused = try_guard::<(First, Deputy), FirstAndDeputy, SetMark>(
        &input,
        &SEED,
        InitGate::Deployer,
        op,
        keep,
    )
    .err();
    assert_eq!(
        refused,
        Some(Refusal::of::<Deputy>(AuthorityError::NotThroughSeat))
    );
}

#[test]
fn a_slot_change_names_the_slot_by_the_role_s_name() {
    assert_eq!(
        Refusal::of::<Deputy>(AuthorityError::SlotChanged).text(),
        "lez-authority/deputy E126: a handler must not change the deputy slot"
    );
    assert_eq!(
        Refusal::of::<Deputy>(AuthorityError::NotInstalled).text(),
        "lez-authority/deputy E131: there is no deputy authority"
    );
}

/// Runs `message` at `receiver` of a program with the roles `Rs` and the config `C`.
fn at_with<Rs, C, M>(
    receiver: Actor,
    from: Option<Actor>,
    authorized: bool,
    pre: Option<&C>,
    message: Wire<M>,
    handler: fn(&ReceiveInput, M) -> Response,
) -> Result<Response, Refusal>
where
    Rs: Roles<C, M>,
    C: BorshSerialize + BorshDeserialize + Default,
    M: Gated + BorshSerialize + BorshDeserialize,
{
    let input = ReceiveInput {
        receiver,
        from,
        is_authorized: authorized,
        pre_state: ActorState::from(pre.map(|c| borsh::to_vec(c).unwrap()).unwrap_or_default()),
        message: borsh::to_vec(&message).unwrap(),
    };
    try_guard::<Rs, C, M>(&input, &SEED, InitGate::Deployer, message, handler)
}

/// Runs `message` at `receiver` of a program with the roles `(First, Marking)`.
fn at_marked_with<M>(
    receiver: Actor,
    from: Option<Actor>,
    authorized: bool,
    pre: Option<&RootAndMark>,
    message: Wire<M>,
    handler: fn(&ReceiveInput, M) -> Response,
) -> Result<Response, Refusal>
where
    M: Gated + BorshSerialize + BorshDeserialize,
{
    at_with::<(First, Marking), RootAndMark, M>(receiver, from, authorized, pre, message, handler)
}

fn at_marked(
    receiver: Actor,
    from: Option<Actor>,
    authorized: bool,
    pre: Option<&RootAndMark>,
    message: Wire<SetMark>,
) -> Result<Response, Refusal> {
    at_marked_with(receiver, from, authorized, pre, message, keep)
}

/// From `sender`'s seat to the config of a program with the roles `(First, Marking)`. The
/// config as written back.
fn send_marked(
    sender: AccountId,
    pre: Option<&RootAndMark>,
    message: Wire<SetMark>,
) -> Result<RootAndMark, Refusal> {
    let hop = at_marked(seat_of(sender), None, true, None, message)?;
    let hop: Wire<SetMark> = borsh::from_slice(&hop.sync_messages[0].message).unwrap();
    let response = at_marked(config_actor(), Some(seat_of(sender)), false, pre, hop)?;
    Ok(borsh::from_slice(response.post_state.as_ref().unwrap()).unwrap())
}

fn set_mark_op(mark: u64) -> Wire<SetMark> {
    Wire::op::<Marking>(Op::Own(borsh::to_vec(&mark).unwrap()))
}

/// The first role held by the deployer, the marking role by `KEY`, with the mark at 7.
fn root_and_mark() -> RootAndMark {
    RootAndMark {
        first: held(PROGRAM),
        marking: Marked {
            authority_slot: held(KEY),
            mark: 7,
        },
    }
}

#[test]
fn a_role_s_own_operation_changes_its_slot_and_nothing_else() {
    let mut expected = root_and_mark();
    expected.marking.mark = 9;
    assert_eq!(
        send_marked(KEY, Some(&root_and_mark()), set_mark_op(9)),
        Ok(expected)
    );
}

#[test]
fn a_role_s_own_operation_is_refused_in_its_name() {
    assert_eq!(
        send_marked(OTHER, Some(&root_and_mark()), set_mark_op(9)),
        Err(Refusal::of::<Marking>(AuthorityError::NotHolder))
    );
    let short = Wire::op::<Marking>(Op::Own(vec![9]));
    assert_eq!(
        send_marked(KEY, Some(&root_and_mark()), short),
        Err(Refusal::of::<Marking>(AuthorityError::MessageDoesNotDecode))
    );
}

#[test]
fn a_role_that_defines_no_operations_refuses_one_as_a_message_that_does_not_decode() {
    assert_eq!(
        send_marked(
            PROGRAM,
            Some(&root_and_mark()),
            Wire::op::<First>(Op::Own(Vec::new()))
        ),
        Err(Refusal::of::<First>(AuthorityError::MessageDoesNotDecode))
    );
    assert_eq!(
        send(
            KEY,
            Some(&two(Some(KEY))),
            Wire::op::<Second>(Op::Own(vec![1]))
        ),
        Err(Refusal::of::<Second>(AuthorityError::MessageDoesNotDecode))
    );
}

#[test]
fn before_the_root_role_s_init_no_operation_of_a_role_s_own_runs() {
    for message in [set_mark_op(9), Wire::op::<First>(Op::Own(Vec::new()))] {
        assert_eq!(
            send_marked(KEY, None, message),
            Err(Refusal::of::<First>(AuthorityError::NotInitialized))
        );
    }
}

#[test]
fn a_role_s_own_operation_sent_straight_to_the_config_is_refused_in_its_name() {
    assert_eq!(
        at_marked(
            config_actor(),
            None,
            false,
            Some(&root_and_mark()),
            set_mark_op(9)
        )
        .err(),
        Some(Refusal::of::<Marking>(AuthorityError::NotThroughSeat))
    );
}

#[test]
fn a_role_s_own_operation_follows_the_four_slot_operations_on_the_wire() {
    assert_eq!(borsh::to_vec(&Op::Init).unwrap(), [0]);
    assert_eq!(
        borsh::to_vec(&Op::Nominate { next: KEY }).unwrap(),
        [&[1][..], &[1; 32]].concat()
    );
    assert_eq!(
        borsh::to_vec(&Op::Accept { pda_seed: None }).unwrap(),
        [2, 0]
    );
    assert_eq!(borsh::to_vec(&Op::Renounce).unwrap(), [3]);
    assert_eq!(borsh::to_vec(&Op::Own(vec![9, 8])).unwrap(), [4, 9, 8]);
    assert_eq!(
        borsh::from_slice::<Op>(&[4, 9, 8]).unwrap(),
        Op::Own(vec![9, 8])
    );
    assert_eq!(borsh::from_slice::<Op>(&[4]).unwrap(), Op::Own(Vec::new()));
    let own = Wire::<u16>::op::<Marking>(Op::Own(vec![9]));
    let bytes = borsh::to_vec(&own).unwrap();
    assert_eq!(bytes.last(), Some(&9));
    assert_eq!(borsh::from_slice::<Wire<u16>>(&bytes).unwrap(), own);
}

// A role whose own operation tries to take its holder part.
struct Seizing;

impl Role for Seizing {
    const NAME: &'static str = "seizing";
    type Slot = Marked;
}

impl<C: HasSlot<Seizing>> Policy<C> for Seizing {
    fn operate(ctx: &Ctx, config: &C, _op: &[u8]) -> Result<Marked, Refusal> {
        let caller = ctx.caller().unwrap();
        Ok(Marked {
            authority_slot: AuthoritySlot::held(Holder {
                account: caller.account(),
                via: caller.via(),
            }),
            mark: HasSlot::<Seizing>::slot(config).mark + 1,
        })
    }
}

impl HasSlot<Seizing> for WithMark {
    fn slot(&self) -> &Marked {
        &self.0
    }
    fn slot_mut(&mut self) -> &mut Marked {
        &mut self.0
    }
}

#[test]
fn a_role_s_own_operation_never_moves_its_holder() {
    for authority_slot in [
        AuthoritySlot::Unset,
        AuthoritySlot::Offered { nominee: KEY },
        held(KEY),
        AuthoritySlot::Renounced,
    ] {
        let mut config = marked(authority_slot.clone());
        let ctx = Ctx {
            caller: Some(caller(OTHER, Some(ELSEWHERE))),
            program: PROGRAM,
            gate: InitGate::Deployer,
        };
        run_op::<Seizing, WithMark>(&ctx, &mut config, Op::Own(Vec::new())).unwrap();
        assert_eq!(
            config,
            WithMark(Marked {
                authority_slot,
                mark: 8
            })
        );
    }
}

/// A program message for `(First, Marking)`: open or needing the first role's holder, and
/// exempt from the marking role's rules or not.
#[derive(BorshSerialize, BorshDeserialize)]
enum Halt {
    Open,
    Gated,
    Exempt,
    GatedExempt,
    ExemptTouch,
}

impl Gated for Halt {
    fn needs<R: Role>(&self) -> bool {
        matches!(self, Halt::Gated | Halt::GatedExempt)
            && std::any::TypeId::of::<R>() == std::any::TypeId::of::<First>()
    }

    fn exempt<R: Role>(&self) -> bool {
        matches!(self, Halt::Exempt | Halt::GatedExempt | Halt::ExemptTouch)
            && std::any::TypeId::of::<R>() == std::any::TypeId::of::<Marking>()
    }
}

/// Keeps the config, but for `ExemptTouch`, which moves the marking role's mark.
fn halt(input: &ReceiveInput, message: Halt) -> Response {
    if let Halt::ExemptTouch = message {
        let mut config: RootAndMark = borsh::from_slice(&input.pre_state).unwrap();
        config.marking.mark += 1;
        return Response::set_state(borsh::to_vec(&config).unwrap());
    }
    Response::keep_state()
}

/// `root_and_mark()` with the mark at zero.
fn halted() -> RootAndMark {
    let mut config = root_and_mark();
    config.marking.mark = 0;
    config
}

/// `message` sent straight to the config.
fn straight(pre: &RootAndMark, message: Halt) -> Result<Response, Refusal> {
    at_marked_with(
        config_actor(),
        None,
        false,
        Some(pre),
        Wire::Program(message),
        halt,
    )
}

/// `message` from `sender`'s seat to the config.
fn halt_through_seat(
    sender: AccountId,
    pre: &RootAndMark,
    message: Halt,
) -> Result<Response, Refusal> {
    let hop = at_marked_with(
        seat_of(sender),
        None,
        true,
        None,
        Wire::Program(message),
        halt,
    )?;
    let hop: Wire<Halt> = borsh::from_slice(&hop.sync_messages[0].message).unwrap();
    at_marked_with(
        config_actor(),
        Some(seat_of(sender)),
        false,
        Some(pre),
        hop,
        halt,
    )
}

#[test]
fn a_role_s_rules_refuse_a_program_message_at_the_config_in_its_name() {
    let refused = Some(Refusal::of::<Marking>(HALTED));
    assert_eq!(straight(&halted(), Halt::Open).err(), refused);
    assert_eq!(
        halt_through_seat(PROGRAM, &halted(), Halt::Gated).err(),
        refused
    );
    assert!(straight(&root_and_mark(), Halt::Open).is_ok());
    assert!(halt_through_seat(PROGRAM, &root_and_mark(), Halt::Gated).is_ok());
    assert_eq!(
        Refusal::of::<Marking>(HALTED).text(),
        "lez-marking-authority/marking E201: the mark is zero"
    );
}

#[test]
fn a_message_exempt_from_a_role_runs_where_its_rules_would_refuse_it() {
    assert!(straight(&halted(), Halt::Exempt).is_ok());
    assert!(halt_through_seat(PROGRAM, &halted(), Halt::GatedExempt).is_ok());
}

#[test]
fn an_exempt_message_still_needs_its_holder_and_leaves_the_slots_alone() {
    assert_eq!(
        halt_through_seat(OTHER, &halted(), Halt::GatedExempt).err(),
        Some(Refusal::of::<First>(AuthorityError::NotHolder))
    );
    assert_eq!(
        straight(&halted(), Halt::ExemptTouch).err(),
        Some(Refusal::of::<Marking>(AuthorityError::SlotChanged))
    );
}

#[test]
fn the_holders_come_before_a_role_s_rules() {
    assert_eq!(
        halt_through_seat(OTHER, &halted(), Halt::Gated).err(),
        Some(Refusal::of::<First>(AuthorityError::NotHolder))
    );
}

#[test]
fn a_role_s_rules_never_see_a_management_operation() {
    let mut expected = halted();
    expected.marking.mark = 9;
    assert_eq!(
        send_marked(KEY, Some(&halted()), set_mark_op(9)),
        Ok(expected)
    );
    let nominate = Wire::op::<First>(Op::Nominate { next: OTHER });
    let nominated = send_marked(PROGRAM, Some(&halted()), nominate).unwrap();
    assert_eq!(nominated.first.nominee(), Some(OTHER));
}

#[test]
fn before_init_an_exempt_message_at_the_config_is_refused_too() {
    assert_eq!(
        at_marked_with(
            config_actor(),
            None,
            false,
            None,
            Wire::Program(Halt::Exempt),
            halt
        )
        .err(),
        Some(Refusal::of::<First>(AuthorityError::NotInitialized))
    );
}

fn never(_input: &ReceiveInput, _message: SetMark) -> Response {
    unreachable!("a refused message never reaches the handler")
}

#[test]
fn a_role_s_rules_answer_before_the_handler_runs() {
    let refused = Some(Refusal::of::<Marking>(HALTED));
    assert_eq!(
        at_config_with::<(First, Marking), _>(&halted(), 7, never),
        refused
    );
    assert_eq!(
        at_config_with::<(First, Marking), _>(&halted(), 8, set_second_mark),
        refused
    );
}

#[test]
fn an_exempt_message_at_a_config_that_does_not_decode_is_refused() {
    let message = Wire::Program(Halt::Exempt);
    let input = ReceiveInput {
        receiver: config_actor(),
        from: None,
        is_authorized: false,
        pre_state: ActorState::from(vec![9]),
        message: borsh::to_vec(&message).unwrap(),
    };
    assert_eq!(
        try_guard::<(First, Marking), RootAndMark, Halt>(
            &input,
            &SEED,
            InitGate::Deployer,
            message,
            halt
        )
        .err(),
        Some(Refusal::of::<First>(AuthorityError::ConfigDoesNotDecode))
    );
}

// A role with rules of its own but no REFUSES_MESSAGES, so the guard never asks them.
struct Halting;

impl Role for Halting {
    const NAME: &'static str = "halting";
    type Slot = Marked;
}

/// What the halting role answers to a program message while its mark is zero.
const STOPPED: AuthorityError = AuthorityError::Role {
    krate: "lez-halting-authority",
    code: 201,
    sentence: "the halt is on",
};

impl<C: HasSlot<Halting>> Policy<C> for Halting {
    fn may_run(_ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        if HasSlot::<Halting>::slot(config).mark == 0 {
            return Err(Refusal::of::<Halting>(STOPPED));
        }
        Ok(())
    }
}

/// The slots of the marking role and of the halting role.
#[derive(Debug, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
struct Marks {
    marking: Marked,
    halting: Marked,
}

impl HasSlot<Marking> for Marks {
    fn slot(&self) -> &Marked {
        &self.marking
    }
    fn slot_mut(&mut self) -> &mut Marked {
        &mut self.marking
    }
}

impl HasSlot<Halting> for Marks {
    fn slot(&self) -> &Marked {
        &self.halting
    }
    fn slot_mut(&mut self) -> &mut Marked {
        &mut self.halting
    }
}

/// The marking role held by `KEY` and the halting role by the deployer, with their marks.
fn marks(marking: u64, halting: u64) -> Marks {
    Marks {
        marking: Marked {
            authority_slot: held(KEY),
            mark: marking,
        },
        halting: Marked {
            authority_slot: held(PROGRAM),
            mark: halting,
        },
    }
}

/// A program message for the marking and halting roles: open, exempt from one of them, or
/// needing the marking role's holder.
#[derive(BorshSerialize, BorshDeserialize)]
enum Ask {
    Open,
    ExemptMarking,
    ExemptHalting,
    GatedMarking,
}

impl Gated for Ask {
    fn needs<R: Role>(&self) -> bool {
        matches!(self, Ask::GatedMarking)
            && std::any::TypeId::of::<R>() == std::any::TypeId::of::<Marking>()
    }

    fn exempt<R: Role>(&self) -> bool {
        let role = std::any::TypeId::of::<R>();
        match self {
            Ask::ExemptMarking => role == std::any::TypeId::of::<Marking>(),
            Ask::ExemptHalting => role == std::any::TypeId::of::<Halting>(),
            _ => false,
        }
    }
}

/// `message` sent straight to the config of a program with the roles `Rs`.
fn ask<Rs: Roles<Marks, Ask>>(pre: &Marks, message: Ask) -> Result<Response, Refusal> {
    at_with::<Rs, Marks, Ask>(
        config_actor(),
        None,
        false,
        Some(pre),
        Wire::Program(message),
        keep,
    )
}

/// `message` from `sender`'s seat to the config of a program with the roles `Both`.
fn ask_through_seat<Both: Roles<Marks, Ask>>(
    sender: AccountId,
    pre: &Marks,
    message: Ask,
) -> Result<Response, Refusal> {
    let hop = at_with::<Both, Marks, Ask>(
        seat_of(sender),
        None,
        true,
        None,
        Wire::Program(message),
        keep,
    )?;
    let hop: Wire<Ask> = borsh::from_slice(&hop.sync_messages[0].message).unwrap();
    at_with::<Both, Marks, Ask>(
        config_actor(),
        Some(seat_of(sender)),
        false,
        Some(pre),
        hop,
        keep,
    )
}

#[test]
fn a_lone_role_s_rules_refuse_every_program_message_but_an_exempt_one() {
    assert_eq!(
        ask::<(Marking,)>(&marks(0, 7), Ask::Open).err(),
        Some(Refusal::of::<Marking>(HALTED))
    );
    assert!(ask::<(Marking,)>(&marks(0, 7), Ask::ExemptMarking).is_ok());
    assert!(ask::<(Marking,)>(&marks(7, 0), Ask::Open).is_ok());
}

#[test]
fn rules_a_role_does_not_declare_are_never_asked() {
    assert!(ask::<(Halting,)>(&marks(7, 0), Ask::Open).is_ok());
    assert!(ask::<(Marking, Halting)>(&marks(7, 0), Ask::Open).is_ok());
    assert_eq!(
        ask::<(Marking, Halting)>(&marks(0, 0), Ask::Open).err(),
        Some(Refusal::of::<Marking>(HALTED))
    );
    assert_eq!(
        ask::<(Halting, Marking)>(&marks(0, 0), Ask::Open).err(),
        Some(Refusal::of::<Marking>(HALTED))
    );
}

#[test]
fn an_exemption_from_another_role_lifts_nothing() {
    assert_eq!(
        ask::<(Marking, Halting)>(&marks(0, 0), Ask::ExemptHalting).err(),
        Some(Refusal::of::<Marking>(HALTED))
    );
    assert!(ask::<(Marking, Halting)>(&marks(0, 0), Ask::ExemptMarking).is_ok());
}

#[test]
fn a_role_s_rules_refuse_its_own_holder_too() {
    assert_eq!(
        ask_through_seat::<(Marking, Halting)>(KEY, &marks(0, 7), Ask::GatedMarking).err(),
        Some(Refusal::of::<Marking>(HALTED))
    );
    assert!(ask_through_seat::<(Marking, Halting)>(KEY, &marks(7, 7), Ask::GatedMarking).is_ok());
}

#[test]
fn a_role_s_rules_see_who_sent_a_message_through_its_seat() {
    let mut held_by_other = marks(7, 7);
    held_by_other.marking.authority_slot = held(OTHER);
    assert_eq!(
        ask_through_seat::<(Marking, Halting)>(OTHER, &held_by_other, Ask::GatedMarking).err(),
        Some(Refusal::of::<Marking>(BARRED))
    );
    assert_eq!(
        ask_through_seat::<(Halting, Marking)>(OTHER, &held_by_other, Ask::GatedMarking).err(),
        Some(Refusal::of::<Marking>(BARRED))
    );
    assert!(ask::<(Marking, Halting)>(&held_by_other, Ask::Open).is_ok());
}

#[test]
fn a_marked_role_is_listed_only_under_its_exact_name() {
    assert!(listed(&["admin", "freeze"], &["freeze", "admin"]));
    assert!(listed(&["admin"], &[]));
    assert!(!listed(&["admin"], &["freeze"]));
    assert!(!listed(&["admin"], &["admi"]));
    assert!(!listed(&["admin"], &["admins"]));
    assert!(!listed(&["admin", "freeze"], &["admin", "pauser"]));
}
