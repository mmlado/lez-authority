use std::collections::{BTreeMap, BTreeSet};

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, Actor, ActorState};
use lee_core::program::{AsyncMessage, PdaSeed, ReceiveInput, Response, SyncMessage, Transition};
use lez_authority_world::{Failure, Transaction, World};

use super::*;
use crate::guard::{listed, run_op};

mod clearance;
mod layers;
mod own_operations;
mod refusing;
mod role_slots;
mod routing;
mod sends;
mod slot;
mod watching;

const KEY: AccountId = AccountId::new([1; 32]);
const OTHER: AccountId = AccountId::new([2; 32]);
const GOVERNANCE: AccountId = AccountId::new([6; 32]);
const PROGRAM: AccountId = AccountId::new([0xA0; 32]);
const ELSEWHERE: AccountId = AccountId::new([0xB0; 32]);

fn caller(account: AccountId, via: Option<AccountId>) -> Caller {
    Caller {
        account,
        via,
        via_known: true,
    }
}

fn held(account: AccountId) -> AuthoritySlot {
    AuthoritySlot::held(Holder { account, via: None })
}

// Two roles shaped like the admin and a freeze authority that answers to it.

struct First;

impl Role for First {
    const NAME: &'static str = "first";
    const PROSE_NAME: &'static str = "first role";
    type Slot = AuthoritySlot;
}

impl<C: HasSlot<First>> Policy<C> for First {}
impl<C: HasSlot<First>> Hooks<C> for First {}

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

impl<C: HasSlot<First> + HasSlot<Second>> Hooks<C> for Second {}

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

/// Every actor's state between transactions.
type States = BTreeMap<Actor, Vec<u8>>;

/// What starts a message that asks another program to send bytes on.
const SEND_ON: [u8; 8] = *b"send on:";

/// A message for another program: send `bytes` on to `to`.
fn sent_on(to: Actor, bytes: &[u8]) -> Vec<u8> {
    borsh::to_vec(&(SEND_ON, to, bytes)).unwrap()
}

/// Any other program. It sends bytes on when its message asks it to, and otherwise does
/// nothing.
fn other_program(input: &ReceiveInput) -> Result<Response, Refusal> {
    let mut response = Response::keep_state();
    if let Ok((SEND_ON, to, bytes)) = borsh::from_slice::<([u8; 8], Actor, Vec<u8>)>(&input.message)
    {
        response.sync_messages.push(SyncMessage {
            to,
            message: bytes,
            pda_seeds: BTreeSet::new(),
        });
    }
    Ok(response)
}

/// One transaction over `pre` in a world whose program is the guard of `Rs` around `handler`:
/// `message` at `root`, signed by `signers`, reaching `reached` beside the config, the actors
/// that hold state and each signer's seat. Every delivery, or the refusal that refuses all of it.
fn deliver<Rs, C, M>(
    pre: &States,
    root: Actor,
    signers: &[AccountId],
    message: Vec<u8>,
    reached: &[Actor],
    handler: fn(&ReceiveInput, M) -> Response,
) -> Result<Vec<Transition>, Refusal>
where
    Rs: Roles<C, M> + 'static,
    C: BorshSerialize + BorshDeserialize + Default + 'static,
    M: Gated + BorshSerialize + BorshDeserialize + 'static,
{
    let guarded = move |input: &ReceiveInput| {
        let wire: Wire<M> = borsh::from_slice(&input.message).unwrap();
        try_guard::<Rs, C, M>(input, &SEED, InitGate::Deployer, wire, handler)
    };
    let mut world = World::new().program(PROGRAM, guarded);
    for other in [ELSEWHERE, GOVERNANCE] {
        world = world.program(other, other_program);
    }
    for (&actor, state) in pre {
        world = world.starting_with(actor, state.clone());
    }
    let mut actors = vec![root, config_actor()];
    actors.extend(pre.keys());
    actors.extend(reached);
    actors.extend(signers.iter().copied().map(seat_of));
    let mut evidence = vec![root.account_id, config_actor().account_id];
    evidence.extend(reached.iter().map(|actor| actor.account_id));
    let transaction = Transaction {
        to: root,
        message,
        actors,
        signers: signers.to_vec(),
        evidence,
    };
    match world.transact(transaction) {
        Ok(outcome) => Ok(outcome.trace),
        Err(Failure::Refused(refusal)) => Err(refusal),
        Err(Failure::Executor(error)) => panic!("the executor refused the transaction: {error}"),
        Err(Failure::UnknownProgram(id)) => panic!("the world holds no program {id}"),
    }
}

/// The states of a program whose config is `config`, or of one with none.
fn states<C: BorshSerialize>(config: Option<&C>) -> States {
    config
        .map(|config| (config_actor(), borsh::to_vec(config).unwrap()))
        .into_iter()
        .collect()
}

/// [`deliver`] with `message` encoded, over the config `pre` holds, or none.
fn run_with<Rs, C, M>(
    pre: Option<&C>,
    root: Actor,
    signers: &[AccountId],
    message: Wire<M>,
    reached: &[Actor],
    handler: fn(&ReceiveInput, M) -> Response,
) -> Result<Vec<Transition>, Refusal>
where
    Rs: Roles<C, M> + 'static,
    C: BorshSerialize + BorshDeserialize + Default + 'static,
    M: Gated + BorshSerialize + BorshDeserialize + 'static,
{
    let message = borsh::to_vec(&message).unwrap();
    deliver::<Rs, C, M>(&states(pre), root, signers, message, reached, handler)
}

/// The first delivery at `actor` in `trace`.
fn step_at(trace: &[Transition], actor: Actor) -> &Transition {
    let step = trace.iter().find(|step| step.input.receiver == actor);
    step.expect("the transaction reached the actor")
}

/// What the guard returned at `actor`, the first time `trace` reached it.
fn response_at(trace: &[Transition], actor: Actor) -> Response {
    step_at(trace, actor).response.clone()
}

/// The actors `message` makes the handler send to, which its transaction must list.
fn named(message: &Wire<Msg>) -> Vec<Actor> {
    match message {
        Wire::Program(Msg::ToTarget(target) | Msg::Pass { to: target, .. }) => {
            vec![Actor::new(*target, PROGRAM)]
        }
        Wire::Program(Msg::ToElsewhere(target)) => vec![Actor::new(*target, ELSEWHERE)],
        _ => Vec::new(),
    }
}

/// [`deliver`] for the roles `(First, Second)`, with `message` as bytes.
fn run_bytes(
    pre: Option<&Two>,
    root: Actor,
    signers: &[AccountId],
    message: Vec<u8>,
    reached: &[Actor],
) -> Result<Vec<Transition>, Refusal> {
    deliver::<(First, Second), Two, Msg>(&states(pre), root, signers, message, reached, handle)
}

/// `message` at `root`, signed by `signers`, over the config `pre` holds, or none.
fn run(
    pre: Option<&Two>,
    root: Actor,
    signers: &[AccountId],
    message: Wire<Msg>,
) -> Result<Vec<Transition>, Refusal> {
    let reached = named(&message);
    run_bytes(
        pre,
        root,
        signers,
        borsh::to_vec(&message).unwrap(),
        &reached,
    )
}

/// From `sender`'s seat to the config. What the config answered.
fn through_seat(
    sender: AccountId,
    pre: Option<&Two>,
    message: Wire<Msg>,
) -> Result<Response, Refusal> {
    let trace = run(pre, seat_of(sender), &[sender], message)?;
    let hop = response_at(&trace, seat_of(sender));
    assert!(hop.post_state.is_none(), "the seat only forwards");
    Ok(response_at(&trace, config_actor()))
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

// A role whose slot holds more than its holder, as a freeze authority's holds what it froze.

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

impl<C: HasSlot<Marking>> Policy<C> for Marking {}

// Its own operation sets the mark, and a zero mark stops program messages, as a freeze does.
impl<C: HasSlot<Marking>> Hooks<C> for Marking {
    const REFUSES_MESSAGES: bool = true;

    fn operate(ctx: &Ctx, config: &C, op: &[u8]) -> Result<(Marked, Vec<SyncMessage>), Refusal> {
        let mark = borsh::from_slice(op)
            .map_err(|_| Refusal::of::<Marking>(AuthorityError::MessageDoesNotDecode))?;
        let slot = HasSlot::<Marking>::slot(config);
        ctx.check::<Marking>(slot.authority_slot())?;
        let slot = Marked {
            authority_slot: slot.authority_slot.clone(),
            mark,
        };
        Ok((slot, Vec::new()))
    }

    fn may_run(ctx: &Ctx, config: &C) -> Result<Vec<SyncMessage>, Refusal> {
        if HasSlot::<Marking>::slot(config).mark == 0 {
            return Err(Refusal::of::<Marking>(HALTED));
        }
        if ctx.caller().is_some_and(|caller| caller.account() == OTHER) {
            return Err(Refusal::of::<Marking>(BARRED));
        }
        Ok(Vec::new())
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

/// `SetMark(mark)` sent straight to the config `pre` of a program with the roles `Rs` around
/// `handler`.
fn at_config_with<Rs, C>(
    pre: &C,
    mark: u64,
    handler: fn(&ReceiveInput, SetMark) -> Response,
) -> Option<Refusal>
where
    Rs: Roles<C, SetMark> + 'static,
    C: BorshSerialize + BorshDeserialize + Default + 'static,
{
    let message = Wire::Program(SetMark(mark));
    run_with::<Rs, C, SetMark>(Some(pre), config_actor(), &[], message, &[], handler).err()
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

fn keep<M>(_input: &ReceiveInput, _message: M) -> Response {
    Response::keep_state()
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

/// From `sender`'s seat to the config of a program with the roles `(First, Marking)`. The
/// config as written back.
fn send_marked(
    sender: AccountId,
    pre: Option<&RootAndMark>,
    message: Wire<SetMark>,
) -> Result<RootAndMark, Refusal> {
    let seat = seat_of(sender);
    let trace = run_with::<(First, Marking), _, _>(pre, seat, &[sender], message, &[], keep)?;
    let response = response_at(&trace, config_actor());
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

/// A program message for `(First, Marking)`: open or needing the first role's holder, and
/// exempt from the marking role's rules or not.
#[derive(BorshSerialize, BorshDeserialize)]
enum Halt {
    Open,
    Gated,
    Exempt,
    GatedExempt,
    ExemptTouch,
    FromConfig,
    FromConfigExempt,
    WriteAndSend,
    Pass { to: Actor, bytes: Vec<u8> },
}

impl Gated for Halt {
    fn needs<R: Role>(&self) -> bool {
        matches!(self, Halt::Gated | Halt::GatedExempt)
            && std::any::TypeId::of::<R>() == std::any::TypeId::of::<First>()
    }

    fn only_from_config(&self) -> bool {
        matches!(self, Halt::FromConfig | Halt::FromConfigExempt)
    }

    fn exempt<R: Role>(&self) -> bool {
        matches!(
            self,
            Halt::Exempt
                | Halt::GatedExempt
                | Halt::ExemptTouch
                | Halt::FromConfigExempt
                | Halt::Pass { .. }
        ) && std::any::TypeId::of::<R>() == std::any::TypeId::of::<Marking>()
    }
}

/// Keeps the config, but for `ExemptTouch`, which moves the marking role's mark. `Pass` sends
/// its bytes on, so a test chooses the sender of a message.
fn halt(input: &ReceiveInput, message: Halt) -> Response {
    if let Halt::ExemptTouch = message {
        let mut config: RootAndMark = borsh::from_slice(&input.pre_state).unwrap();
        config.marking.mark += 1;
        return Response::set_state(borsh::to_vec(&config).unwrap());
    }
    if let Halt::WriteAndSend = message {
        return Response::set_state(vec![7]).send_sync(seat_of(OTHER), &Wire::Program(Halt::Open));
    }
    if let Halt::Pass { to, bytes } = message {
        let mut response = Response::keep_state();
        response.sync_messages.push(SyncMessage {
            to,
            message: bytes,
            pda_seeds: BTreeSet::new(),
        });
        return response;
    }
    Response::keep_state()
}

/// `root_and_mark()` with the mark at zero.
fn halted() -> RootAndMark {
    let mut config = root_and_mark();
    config.marking.mark = 0;
    config
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

impl<C: HasSlot<Halting>> Policy<C> for Halting {}

impl<C: HasSlot<Halting>> Hooks<C> for Halting {
    fn may_run(_ctx: &Ctx, config: &C) -> Result<Vec<SyncMessage>, Refusal> {
        if HasSlot::<Halting>::slot(config).mark == 0 {
            return Err(Refusal::of::<Halting>(STOPPED));
        }
        Ok(Vec::new())
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
