//! The guard: where each message runs, and what is checked on the way, over a program's
//! roles.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, Actor};
use lee_core::program::{PdaSeed, ReceiveInput, Response, SyncMessage};
use lez_authority_core::{AuthorityError, HasSlot, InitGate, Refusal, Role};

use crate::hooks::{self, Hooks};
use crate::policy::{self, Caller, Ctx};
use crate::wire::{self, Clearance, Forwarded, Gated, LIBRARY_TAG, LibraryMessage, Op, Wire};

/// The config account of a program with this seed.
#[must_use]
pub fn config_account(program: AccountId, seed: &PdaSeed) -> AccountId {
    AccountId::for_public_pda(&program, seed)
}

/// A label padded with zeros to the 32 bytes a seed holds.
///
/// # Panics
/// At compile time, for a label longer than 32 bytes.
#[must_use]
pub const fn seed(label: &[u8]) -> PdaSeed {
    assert!(label.len() <= 32, "seed label longer than 32 bytes");
    let mut bytes = [0u8; 32];
    let mut i = 0;
    while i < label.len() {
        bytes[i] = label[i];
        i += 1;
    }
    PdaSeed::new(bytes)
}

/// Runs `handler` behind the guard. The [`guard`](macro@crate::guard) attribute writes this call,
/// and a program may write it itself. `Rs` is the program's roles, `(A,)` or `(A, B)`, `C`
/// the config the program keeps on the config account, and `gate` says who may install the
/// root role.
///
/// - A library operation goes to the config through the sender's seat, and the guard runs it
///   there: `Accept` for the nominee alone, the other holder operations under the role's
///   [`Policy`](policy::Policy), and one the role defines for itself through
///   [`Hooks::operate`]. The root role's `Init` creates the config.
/// - A message that needs a holder goes the same way. At the config the guard checks the
///   holder, asks [`Hooks::may_run`] of the role that refuses program messages, unless the
///   message is exempt from it, runs the handler, and refuses a result that changed a slot.
/// - A message only the config sends runs only when it came from the config account.
/// - A program message handled at an actor other than the config, when a role refuses program
///   messages and the message is not exempt from it, asks the config for clearance: the guard
///   adds the request to the handler's response, and the config answers it under that role's
///   rules in the same transaction.
/// - A role's own message, which only the config sends, runs only at the role's own actor for
///   its key, [`own_actor`](hooks::own_actor), under that role's [`Hooks::receive`], over that
///   actor's state, and never reaches the handler.
/// - Anything else runs where it was sent, as in a program without the guard. At the config
///   that needs an initialized config and the consent of the role that refuses program
///   messages, unless the message is exempt from it, and the slots are checked afterwards.
///
/// Wherever the handler runs, the guard first asks each role that sets
/// [`Hooks::WATCHES_STATE`] whether the handler may run over the actor's state, through
/// [`Hooks::may_handle`]. At the config that comes after the holders and the consent. After the
/// handler the guard refuses a response that sends the config a library message, which the
/// config would take for a hop from that actor. It also refuses one that sends a role's own
/// message to an actor of the program (E140), since a handler at the config that sent one would
/// pose as the role at the actor it reaches. Then it asks the same roles whether the handler's
/// write may stand, through [`Hooks::may_write`].
///
/// The refusing role's consent, and a role's own operation, may come with sync messages, which
/// the config sends in the same transaction: the consent's ahead of the handler's own. A message
/// to an actor of the program among them is refused (E139), since every actor of the program
/// takes a message from the config as already gated, but for the role's own message to an actor
/// other than the config.
///
/// The handler of a message that came through a seat sees `input.from` as the sender's own
/// actor and `input.message` as the hop. A PDA holder's grant comes with the call, so every
/// program the handler calls receives it too.
///
/// # Panics
/// On every refusal, with [`Refusal::text`] as the text.
pub fn guard<Rs, C, M>(
    input: &ReceiveInput,
    seed: &PdaSeed,
    gate: InitGate,
    message: Wire<M>,
    handler: impl FnOnce(&ReceiveInput, M) -> Response,
) -> Response
where
    Rs: Roles<C, M>,
    C: BorshSerialize + BorshDeserialize + Default,
    M: Gated + BorshDeserialize,
{
    match try_guard::<Rs, C, M>(input, seed, gate, message, handler) {
        Ok(response) => response,
        Err(refusal) => panic!("{}", refusal.text()),
    }
}

/// [`guard()`] with the guard's refusals as a value. The handler's own panics stay panics.
///
/// # Errors
/// See [`Refusal`].
pub fn try_guard<Rs, C, M>(
    input: &ReceiveInput,
    seed: &PdaSeed,
    gate: InitGate,
    message: Wire<M>,
    handler: impl FnOnce(&ReceiveInput, M) -> Response,
) -> Result<Response, Refusal>
where
    Rs: Roles<C, M>,
    C: BorshSerialize + BorshDeserialize + Default,
    M: Gated + BorshDeserialize,
{
    const {
        assert!(
            !Rs::ROOT_BY_NOMINATION_ONLY,
            "the first role creates the config with its Init, so its holder cannot be appointed only by nomination"
        );
        assert!(
            Rs::NAMES_DISTINCT,
            "the guard's roles need distinct names, since a client's operation names its role"
        );
        assert!(
            Rs::AT_MOST_ONE_REFUSES,
            "one of the guard's roles at most may refuse program messages, since a request for clearance names no role"
        );
        assert!(
            Rs::MARKS_LISTED,
            "a marker on the message enum names a role that is not among the guard's roles"
        );
        assert!(
            Rs::EXEMPTIONS_REFUSE,
            "an exempt marker names a role that refuses no program messages"
        );
    }
    let refuse = |error| Refusal {
        role: Rs::ROOT,
        prose: Rs::ROOT_PROSE,
        error,
    };
    let program = input.receiver.program_account_id;
    let config_actor = Actor::new(config_account(program, seed), program);
    let at_config = input.receiver == config_actor;
    let from_config = input.from == Some(config_actor);
    let handler = |input: &ReceiveInput, message: M| {
        let ctx = Ctx::without_caller(config_actor, gate);
        if Rs::WATCHES_STATE {
            Rs::may_handle(&ctx, &input.pre_state)?;
        }
        let response = handler(input, message);
        // The config trusts a hop from any actor of this program, so a handler must not send one.
        if sends_library_message(&response, config_actor) {
            return Err(refuse(AuthorityError::LibrarySend));
        }
        // An actor takes a role's own message from the config as the role's.
        if hooks::sends_own_message(&response, program) {
            return Err(refuse(AuthorityError::HandlerSendsOwnMessage));
        }
        if Rs::WATCHES_STATE {
            Rs::may_write(&ctx, &input.pre_state, response.post_state.as_deref())?;
        }
        Ok(response)
    };
    // Only the config sees a refusing role's rules, so a message handled elsewhere asks it.
    let cleared = |response: Response, clearance: bool| {
        if !clearance {
            return response;
        }
        let request = Wire::<()>::Library(LibraryMessage::Clearance(Clearance {
            authorized: input.is_authorized,
        }));
        response.send_sync(config_actor, &request)
    };
    match message {
        Wire::Program(message) => {
            if message.only_from_config() {
                if at_config || !from_config {
                    return Err(refuse(AuthorityError::NotFromConfig));
                }
                let clearance = Rs::needs_clearance(&message);
                return handler(input, message).map(|response| cleared(response, clearance));
            }
            if !at_config {
                if from_config || !Rs::needs_any(&message) {
                    let clearance = Rs::needs_clearance(&message);
                    return handler(input, message).map(|response| cleared(response, clearance));
                }
                return seat(input, config_actor).map_err(refuse);
            }
            at_the_config::<Rs, C, M>(input, config_actor, gate, None, message, handler)
        }
        Wire::Library(LibraryMessage::Op { role, .. }) => {
            let (role, prose) = Rs::index(&role).map_or((Rs::ROOT, Rs::ROOT_PROSE), Rs::names);
            let refuse = |error| Refusal { role, prose, error };
            if at_config {
                return Err(refuse(AuthorityError::NotThroughSeat));
            }
            seat(input, config_actor).map_err(refuse)
        }
        Wire::Library(LibraryMessage::Clearance(request)) => {
            if !at_config {
                return Err(refuse(AuthorityError::ClearanceNotAtConfig));
            }
            let from = input
                .from
                .filter(|_| input.from_own_program())
                .ok_or_else(|| refuse(AuthorityError::ClearanceFromOutside))?;
            answer_clearance::<Rs, C, M>(input, config_actor, gate, from, request)
        }
        Wire::Library(LibraryMessage::Own { role, key, message }) => {
            if at_config || !from_config {
                return Err(refuse(AuthorityError::NotFromConfig));
            }
            let role =
                Rs::index(&role).ok_or_else(|| refuse(AuthorityError::MessageDoesNotDecode))?;
            let ctx = Ctx::without_caller(config_actor, gate);
            let state = Rs::receive(&ctx, role, &key, input.receiver, &input.pre_state, &message)?;
            Ok(state.map_or_else(Response::keep_state, Response::set_state))
        }
        Wire::Library(LibraryMessage::Forwarded(hop)) => {
            if !at_config {
                return Err(refuse(AuthorityError::ForwardedNotAtConfig));
            }
            if !input.from_own_program() {
                return Err(refuse(AuthorityError::NotOwnProgram));
            }
            let from = input
                .from
                .ok_or_else(|| refuse(AuthorityError::NotOwnProgram))?;
            let caller = Caller {
                account: from.account_id,
                via: hop.via,
                via_known: true,
            };
            let inner: Wire<M> = borsh::from_slice(&hop.inner)
                .map_err(|_| refuse(AuthorityError::MessageDoesNotDecode))?;
            match inner {
                Wire::Library(LibraryMessage::Op { role, op }) => {
                    let role = Rs::index(&role)
                        .ok_or_else(|| refuse(AuthorityError::MessageDoesNotDecode))?;
                    manage::<Rs, C, M>(input, config_actor, gate, caller, role, op)
                }
                Wire::Library(
                    LibraryMessage::Forwarded(_)
                    | LibraryMessage::Clearance(_)
                    | LibraryMessage::Own { .. },
                ) => Err(refuse(AuthorityError::MessageDoesNotDecode)),
                Wire::Program(message) => {
                    if message.only_from_config() {
                        return Err(refuse(AuthorityError::NotFromConfig));
                    }
                    at_the_config::<Rs, C, M>(
                        input,
                        config_actor,
                        gate,
                        Some(caller),
                        message,
                        handler,
                    )
                }
            }
        }
    }
}

/// A management operation at the config, which the root role's `Init` may create.
fn manage<Rs, C, M>(
    input: &ReceiveInput,
    config_actor: Actor,
    gate: InitGate,
    caller: Caller,
    role: usize,
    op: Op,
) -> Result<Response, Refusal>
where
    Rs: Roles<C, M>,
    C: BorshSerialize + BorshDeserialize + Default,
{
    let refuse = |error| Refusal {
        role: Rs::ROOT,
        prose: Rs::ROOT_PROSE,
        error,
    };
    let pre: &[u8] = &input.pre_state;
    let mut config: C = if pre.is_empty() {
        if role != 0 || op != Op::Init {
            return Err(refuse(AuthorityError::NotInitialized));
        }
        C::default()
    } else {
        borsh::from_slice(pre).map_err(|_| refuse(AuthorityError::ConfigDoesNotDecode))?
    };
    let ctx = Ctx {
        caller: Some(caller),
        config: config_actor,
        gate,
    };
    let sends = Rs::run_op(&ctx, &mut config, role, op)?;
    let bytes = borsh::to_vec(&config).map_err(|_| refuse(AuthorityError::ConfigDoesNotDecode))?;
    let mut response = Response::set_state(bytes);
    response.sync_messages = sends;
    Ok(response)
}

/// A program message at the config: the declared holders, the refusing role's consent, the
/// handler, then the slots. The consent's sync messages go ahead of the handler's.
fn at_the_config<Rs, C, M>(
    input: &ReceiveInput,
    config_actor: Actor,
    gate: InitGate,
    caller: Option<Caller>,
    message: M,
    handler: impl FnOnce(&ReceiveInput, M) -> Result<Response, Refusal>,
) -> Result<Response, Refusal>
where
    Rs: Roles<C, M>,
    C: BorshSerialize + BorshDeserialize + Default,
    M: Gated,
{
    let refuse = |error| Refusal {
        role: Rs::ROOT,
        prose: Rs::ROOT_PROSE,
        error,
    };
    let pre: &[u8] = &input.pre_state;
    if pre.is_empty() {
        return Err(refuse(AuthorityError::NotInitialized));
    }
    let config: C =
        borsh::from_slice(pre).map_err(|_| refuse(AuthorityError::ConfigDoesNotDecode))?;
    let ctx = Ctx {
        caller,
        config: config_actor,
        gate,
    };
    Rs::gate(&ctx, &config, &message)?;
    let mut sends = Rs::may_run(&ctx, &config, &message)?;
    let mut response = handler(input, message)?;
    if let Some(post) = &response.post_state {
        let post: &[u8] = post;
        let after: C =
            borsh::from_slice(post).map_err(|_| refuse(AuthorityError::ConfigDoesNotDecode))?;
        Rs::unchanged(&after, &config)?;
    }
    sends.append(&mut response.sync_messages);
    response.sync_messages = sends;
    Ok(response)
}

/// A request for clearance at the config: the refusing role's answer, with no handler. The
/// caller is the sending actor's account when it authorized the transaction.
fn answer_clearance<Rs, C, M>(
    input: &ReceiveInput,
    config_actor: Actor,
    gate: InitGate,
    from: Actor,
    request: Clearance,
) -> Result<Response, Refusal>
where
    Rs: Roles<C, M>,
    C: BorshSerialize + BorshDeserialize + Default,
{
    let refuse = |error| Refusal {
        role: Rs::ROOT,
        prose: Rs::ROOT_PROSE,
        error,
    };
    let pre: &[u8] = &input.pre_state;
    if pre.is_empty() {
        return Err(refuse(AuthorityError::NotInitialized));
    }
    let config: C =
        borsh::from_slice(pre).map_err(|_| refuse(AuthorityError::ConfigDoesNotDecode))?;
    let ctx = Ctx {
        caller: request.authorized.then_some(Caller {
            account: from.account_id,
            via: None,
            via_known: false,
        }),
        config: config_actor,
        gate,
    };
    let mut response = Response::keep_state();
    response.sync_messages = Rs::clear(&ctx, &config)?;
    Ok(response)
}

/// Hop 1, at the sender's own actor: vouch and forward the message as it arrived.
fn seat(input: &ReceiveInput, config_actor: Actor) -> Result<Response, AuthorityError> {
    if !input.is_authorized {
        return Err(AuthorityError::MissingSignature);
    }
    let hop = Wire::<()>::Library(LibraryMessage::Forwarded(Forwarded {
        via: input.from.map(|from| from.program_account_id),
        inner: input.message.clone(),
    }));
    Ok(Response::keep_state().send_sync(config_actor, &hop))
}

/// Whether `response` sends a library message to `actor`.
fn sends_library_message(response: &Response, actor: Actor) -> bool {
    wire::sent(response).any(|(to, message)| to == actor && message.first() == Some(&LIBRARY_TAG))
}

/// One management operation on `R`'s slot, and the sync messages the config sends with it:
/// `Accept` for the nominee alone, another holder operation under `R`'s policy, or one of `R`'s
/// own.
pub(crate) fn run_op<R, C>(ctx: &Ctx, config: &mut C, op: Op) -> Result<Vec<SyncMessage>, Refusal>
where
    R: Hooks<C>,
    C: HasSlot<R>,
{
    match op {
        Op::Init => policy::init::<R, C>(ctx, config)?,
        Op::Nominate { next } => policy::nominate::<R, C>(ctx, config, next)?,
        Op::Accept { pda_seed } => policy::accept::<R, C>(ctx, config, pda_seed)?,
        Op::Renounce => policy::renounce::<R, C>(ctx, config)?,
        Op::Own(op) => return hooks::operate::<R, C>(ctx, config, &op),
    }
    Ok(Vec::new())
}

/// The handler must leave every slot as it found it.
fn unchanged<R, C>(after: &C, before: &C) -> Result<(), Refusal>
where
    R: Role,
    C: HasSlot<R>,
{
    if HasSlot::<R>::slot(after) == HasSlot::<R>::slot(before) {
        Ok(())
    } else {
        Err(Refusal::of::<R>(AuthorityError::SlotChanged))
    }
}

/// Whether every name in `exempted` is the name of a role that refuses program messages.
const fn exemptions_refuse(names: &[&str], refuses: &[bool], exempted: &[&str]) -> bool {
    let mut e = 0;
    while e < exempted.len() {
        let mut n = 0;
        while n < names.len() && !(refuses[n] && same(names[n], exempted[e])) {
            n += 1;
        }
        if n == names.len() {
            return false;
        }
        e += 1;
    }
    true
}

/// Whether every name in `marked` is one of `names`.
pub(crate) const fn listed(names: &[&str], marked: &[&str]) -> bool {
    let mut m = 0;
    while m < marked.len() {
        let mut n = 0;
        while n < names.len() && !same(names[n], marked[m]) {
            n += 1;
        }
        if n == names.len() {
            return false;
        }
        m += 1;
    }
    true
}

const fn same(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        return false;
    }
    let mut i = 0;
    while i < a.len() {
        if a[i] != b[i] {
            return false;
        }
        i += 1;
    }
    true
}

mod sealed {
    pub trait Sealed {}
    impl<A> Sealed for (A,) {}
    impl<A, B> Sealed for (A, B) {}
}

/// A program's roles, as a tuple: one role, `(A,)`, or two, `(A, B)`. Each role has its own
/// slot in the config, through [`HasSlot`], and its own [`Policy`](policy::Policy) and
/// [`Hooks`]. The first role is the root: its install creates the config, and the guard's own
/// refusals name it.
pub trait Roles<C, M>: sealed::Sealed {
    /// The root role's name.
    const ROOT: &'static str;
    /// The root role's prose name.
    #[doc(hidden)]
    const ROOT_PROSE: &'static str;
    /// Whether the root role's holder is only ever appointed by nomination, which the guard
    /// refuses at compile time.
    #[doc(hidden)]
    const ROOT_BY_NOMINATION_ONLY: bool;
    /// Whether every role the message enum's markers name is one of these, which the guard
    /// checks at compile time.
    #[doc(hidden)]
    const MARKS_LISTED: bool;
    /// Whether the roles have a name each, which the guard checks at compile time.
    #[doc(hidden)]
    const NAMES_DISTINCT: bool;
    /// Whether one role at most refuses program messages, which the guard checks at compile time.
    #[doc(hidden)]
    const AT_MOST_ONE_REFUSES: bool;
    /// Whether every role an `#[exempt]` marker names refuses program messages, which the guard
    /// checks at compile time.
    #[doc(hidden)]
    const EXEMPTIONS_REFUSE: bool;
    /// Whether a role looks at the state around the handler.
    #[doc(hidden)]
    const WATCHES_STATE: bool;
    /// The place in the tuple of the role with this name.
    #[doc(hidden)]
    fn index(name: &str) -> Option<usize>;
    /// The name and the prose name of the role at `role`.
    #[doc(hidden)]
    fn names(role: usize) -> (&'static str, &'static str);
    /// Runs an operation of the role at `role`, and returns the sync messages the config sends
    /// with it.
    #[doc(hidden)]
    fn run_op(ctx: &Ctx, config: &mut C, role: usize, op: Op) -> Result<Vec<SyncMessage>, Refusal>;
    /// Whether the message needs any role's holder.
    #[doc(hidden)]
    fn needs_any(message: &M) -> bool;
    /// Checks the holders the message declares it needs.
    #[doc(hidden)]
    fn gate(ctx: &Ctx, config: &C, message: &M) -> Result<(), Refusal>;
    /// Asks the role that refuses program messages, unless the message is exempt from it, whether
    /// it may run, and returns the sync messages its consent comes with.
    #[doc(hidden)]
    fn may_run(ctx: &Ctx, config: &C, message: &M) -> Result<Vec<SyncMessage>, Refusal>;
    /// Refuses a slot in `after` that differs from the same slot in `before`.
    #[doc(hidden)]
    fn unchanged(after: &C, before: &C) -> Result<(), Refusal>;
    /// Whether a message handled away from the config asks it for clearance: a role refuses
    /// program messages and the message is not exempt from it.
    #[doc(hidden)]
    fn needs_clearance(message: &M) -> bool;
    /// The answer of the role that refuses program messages to a request for clearance, and the
    /// sync messages it comes with.
    #[doc(hidden)]
    fn clear(ctx: &Ctx, config: &C) -> Result<Vec<SyncMessage>, Refusal>;
    /// The role at `role`'s handling of its own message for `key` at the actor `at`, which must
    /// be the role's own actor for `key`, and the actor's new state.
    #[doc(hidden)]
    fn receive(
        ctx: &Ctx,
        role: usize,
        key: &[u8; 32],
        at: Actor,
        state: &[u8],
        message: &[u8],
    ) -> Result<Option<Vec<u8>>, Refusal>;
    /// Asks each role that watches state whether the handler may run over this state.
    #[doc(hidden)]
    fn may_handle(ctx: &Ctx, state: &[u8]) -> Result<(), Refusal>;
    /// Asks each role that watches state whether the handler's write may stand.
    #[doc(hidden)]
    fn may_write(ctx: &Ctx, before: &[u8], after: Option<&[u8]>) -> Result<(), Refusal>;
}

impl<C, M, A> Roles<C, M> for (A,)
where
    A: Hooks<C>,
    C: HasSlot<A>,
    M: Gated,
{
    const ROOT: &'static str = A::NAME;
    const ROOT_PROSE: &'static str = A::PROSE_NAME;
    const ROOT_BY_NOMINATION_ONLY: bool = A::BY_NOMINATION_ONLY;
    const MARKS_LISTED: bool = listed(&[A::NAME], M::MARKED);
    const NAMES_DISTINCT: bool = true;
    const AT_MOST_ONE_REFUSES: bool = true;
    const EXEMPTIONS_REFUSE: bool =
        exemptions_refuse(&[A::NAME], &[A::REFUSES_MESSAGES], M::EXEMPTED);
    const WATCHES_STATE: bool = A::WATCHES_STATE;

    fn index(name: &str) -> Option<usize> {
        (name == A::NAME).then_some(0)
    }

    fn names(_role: usize) -> (&'static str, &'static str) {
        (A::NAME, A::PROSE_NAME)
    }

    fn run_op(
        ctx: &Ctx,
        config: &mut C,
        _role: usize,
        op: Op,
    ) -> Result<Vec<SyncMessage>, Refusal> {
        run_op::<A, C>(ctx, config, op)
    }

    fn needs_any(message: &M) -> bool {
        message.needs::<A>()
    }

    fn gate(ctx: &Ctx, config: &C, message: &M) -> Result<(), Refusal> {
        policy::gate::<A, C, M>(ctx, config, message)
    }

    fn may_run(ctx: &Ctx, config: &C, message: &M) -> Result<Vec<SyncMessage>, Refusal> {
        hooks::may_run::<A, C, M>(ctx, config, message)
    }

    fn unchanged(after: &C, before: &C) -> Result<(), Refusal> {
        unchanged::<A, C>(after, before)
    }

    fn needs_clearance(message: &M) -> bool {
        hooks::asks::<A, C, M>(message)
    }

    fn clear(ctx: &Ctx, config: &C) -> Result<Vec<SyncMessage>, Refusal> {
        hooks::clears::<A, C>(ctx, config)
    }

    fn receive(
        ctx: &Ctx,
        _role: usize,
        key: &[u8; 32],
        at: Actor,
        state: &[u8],
        message: &[u8],
    ) -> Result<Option<Vec<u8>>, Refusal> {
        hooks::receive::<A, C>(ctx, key, at, state, message)
    }

    fn may_handle(ctx: &Ctx, state: &[u8]) -> Result<(), Refusal> {
        hooks::may_handle::<A, C>(ctx, state)
    }

    fn may_write(ctx: &Ctx, before: &[u8], after: Option<&[u8]>) -> Result<(), Refusal> {
        hooks::may_write::<A, C>(ctx, before, after)
    }
}

impl<C, M, A, B> Roles<C, M> for (A, B)
where
    A: Hooks<C>,
    B: Hooks<C>,
    C: HasSlot<A> + HasSlot<B>,
    M: Gated,
{
    const ROOT: &'static str = A::NAME;
    const ROOT_PROSE: &'static str = A::PROSE_NAME;
    const ROOT_BY_NOMINATION_ONLY: bool = A::BY_NOMINATION_ONLY;
    const MARKS_LISTED: bool = listed(&[A::NAME, B::NAME], M::MARKED);
    const NAMES_DISTINCT: bool = !same(A::NAME, B::NAME);
    const AT_MOST_ONE_REFUSES: bool = !(A::REFUSES_MESSAGES && B::REFUSES_MESSAGES);
    const EXEMPTIONS_REFUSE: bool = exemptions_refuse(
        &[A::NAME, B::NAME],
        &[A::REFUSES_MESSAGES, B::REFUSES_MESSAGES],
        M::EXEMPTED,
    );
    const WATCHES_STATE: bool = A::WATCHES_STATE || B::WATCHES_STATE;

    fn index(name: &str) -> Option<usize> {
        if name == A::NAME {
            Some(0)
        } else {
            (name == B::NAME).then_some(1)
        }
    }

    fn names(role: usize) -> (&'static str, &'static str) {
        if role == 0 {
            (A::NAME, A::PROSE_NAME)
        } else {
            (B::NAME, B::PROSE_NAME)
        }
    }

    fn run_op(ctx: &Ctx, config: &mut C, role: usize, op: Op) -> Result<Vec<SyncMessage>, Refusal> {
        if role == 0 {
            run_op::<A, C>(ctx, config, op)
        } else {
            run_op::<B, C>(ctx, config, op)
        }
    }

    fn needs_any(message: &M) -> bool {
        message.needs::<A>() || message.needs::<B>()
    }

    fn gate(ctx: &Ctx, config: &C, message: &M) -> Result<(), Refusal> {
        policy::gate::<A, C, M>(ctx, config, message)?;
        policy::gate::<B, C, M>(ctx, config, message)
    }

    fn may_run(ctx: &Ctx, config: &C, message: &M) -> Result<Vec<SyncMessage>, Refusal> {
        let mut sends = hooks::may_run::<A, C, M>(ctx, config, message)?;
        sends.extend(hooks::may_run::<B, C, M>(ctx, config, message)?);
        Ok(sends)
    }

    fn unchanged(after: &C, before: &C) -> Result<(), Refusal> {
        unchanged::<A, C>(after, before)?;
        unchanged::<B, C>(after, before)
    }

    fn needs_clearance(message: &M) -> bool {
        hooks::asks::<A, C, M>(message) || hooks::asks::<B, C, M>(message)
    }

    fn clear(ctx: &Ctx, config: &C) -> Result<Vec<SyncMessage>, Refusal> {
        let mut sends = hooks::clears::<A, C>(ctx, config)?;
        sends.extend(hooks::clears::<B, C>(ctx, config)?);
        Ok(sends)
    }

    fn receive(
        ctx: &Ctx,
        role: usize,
        key: &[u8; 32],
        at: Actor,
        state: &[u8],
        message: &[u8],
    ) -> Result<Option<Vec<u8>>, Refusal> {
        if role == 0 {
            hooks::receive::<A, C>(ctx, key, at, state, message)
        } else {
            hooks::receive::<B, C>(ctx, key, at, state, message)
        }
    }

    fn may_handle(ctx: &Ctx, state: &[u8]) -> Result<(), Refusal> {
        hooks::may_handle::<A, C>(ctx, state)?;
        hooks::may_handle::<B, C>(ctx, state)
    }

    fn may_write(ctx: &Ctx, before: &[u8], after: Option<&[u8]>) -> Result<(), Refusal> {
        hooks::may_write::<A, C>(ctx, before, after)?;
        hooks::may_write::<B, C>(ctx, before, after)
    }
}
