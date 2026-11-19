//! A program's roles as a tuple, the management operations on their slots, and the checks the guard
//! runs through it.

use lee_core::account::AccountId;
use lee_core::program::SyncMessage;

use crate::*;

/// One management operation on `R`'s slot, under `R`'s policy, and the sync messages the config
/// sends with it.
pub(crate) fn run_op<R, C>(ctx: &Ctx, config: &mut C, op: Op) -> Result<Vec<SyncMessage>, Refusal>
where
    R: Policy<C>,
    C: HasSlot<R>,
{
    let refuse = Refusal::of::<R>;
    match op {
        Op::Init => {
            if R::BY_NOMINATION_ONLY {
                return Err(refuse(AuthorityError::ByNominationOnly));
            }
            R::may_install(ctx, config)?;
            let caller = *ctx.vouched::<R>()?;
            let slot = HasSlot::<R>::slot_mut(config).authority_slot_mut();
            let nominee = match slot {
                AuthoritySlot::Unset => None,
                AuthoritySlot::Offered { nominee } => Some(*nominee),
                AuthoritySlot::Held { .. } => return Err(refuse(AuthorityError::AlreadyInstalled)),
                AuthoritySlot::Renounced => return Err(refuse(AuthorityError::Renounced)),
            };
            *slot = AuthoritySlot::Held {
                holder: Holder {
                    account: caller.account,
                    via: caller.via,
                },
                nominee,
            };
        }
        Op::Nominate { next } => {
            R::may_nominate(ctx, config)?;
            let offer = (next != AccountId::default()).then_some(next);
            let slot = HasSlot::<R>::slot_mut(config).authority_slot_mut();
            match slot {
                AuthoritySlot::Held { nominee, .. } => *nominee = offer,
                AuthoritySlot::Unset | AuthoritySlot::Offered { .. } => {
                    *slot = offer.map_or(AuthoritySlot::Unset, |nominee| AuthoritySlot::Offered {
                        nominee,
                    });
                }
                AuthoritySlot::Renounced => return Err(refuse(AuthorityError::Renounced)),
            }
        }
        Op::Accept { pda_seed } => {
            let caller = *ctx.vouched::<R>()?;
            let slot = HasSlot::<R>::slot_mut(config).authority_slot_mut();
            let pending = match slot {
                AuthoritySlot::Unset => None,
                AuthoritySlot::Renounced => return Err(refuse(AuthorityError::Renounced)),
                AuthoritySlot::Held { nominee, .. } => *nominee,
                AuthoritySlot::Offered { nominee } => Some(*nominee),
            };
            let Some(nominee) = pending else {
                return Err(refuse(AuthorityError::NoNomination));
            };
            if nominee != caller.account {
                return Err(refuse(AuthorityError::NotNominee));
            }
            match (caller.via, pda_seed) {
                (None, None) => {}
                (Some(program), Some(seed))
                    if AccountId::for_public_pda(&program, &seed) == caller.account => {}
                _ => return Err(refuse(AuthorityError::SeedProofFailed)),
            }
            *slot = AuthoritySlot::held(Holder {
                account: caller.account,
                via: caller.via,
            });
        }
        Op::Renounce => {
            R::may_renounce(ctx, config)?;
            *HasSlot::<R>::slot_mut(config).authority_slot_mut() = R::renounced();
        }
        Op::Own(op) => {
            let holder = HasSlot::<R>::slot(config).authority_slot().clone();
            let (mut slot, sends) = R::operate(ctx, config, &op)?;
            let sends = sendable::<R>(ctx, sends)?;
            // The holder moves only through the four operations above, whatever the role returns.
            *slot.authority_slot_mut() = holder;
            *HasSlot::<R>::slot_mut(config) = slot;
            return Ok(sends);
        }
    }
    Ok(Vec::new())
}

/// Role `R`'s sync messages, unless one goes to an actor of the program.
fn sendable<R: Role>(ctx: &Ctx, sends: Vec<SyncMessage>) -> Result<Vec<SyncMessage>, Refusal> {
    // Every actor of the program takes a message from the config as already gated.
    let in_program =
        |sent: &SyncMessage| sent.to.program_account_id == ctx.config.program_account_id;
    if sends.iter().any(in_program) {
        return Err(Refusal::of::<R>(AuthorityError::RoleSendsInProgram));
    }
    Ok(sends)
}

/// The holder a message declares it needs, checked before the handler.
fn gate<R, C, M>(ctx: &Ctx, config: &C, message: &M) -> Result<(), Refusal>
where
    R: Role,
    C: HasSlot<R>,
    M: Gated,
{
    if message.needs::<R>() {
        ctx.check::<R>(HasSlot::<R>::slot(config).authority_slot())?;
    }
    Ok(())
}

/// Role `R`'s consent to a program message at the config, unless the role declares no refusal or
/// the message is exempt from it.
fn may_run<R, C, M>(ctx: &Ctx, config: &C, message: &M) -> Result<Vec<SyncMessage>, Refusal>
where
    R: Policy<C>,
    C: HasSlot<R>,
    M: Gated,
{
    if !R::REFUSES_MESSAGES || message.exempt::<R>() {
        return Ok(Vec::new());
    }
    sendable::<R>(ctx, R::may_run(ctx, config)?)
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

/// Whether role `R` refuses program messages and `message` is not exempt from it.
fn asks<R, C, M>(message: &M) -> bool
where
    R: Policy<C>,
    C: HasSlot<R>,
    M: Gated,
{
    R::REFUSES_MESSAGES && !message.exempt::<R>()
}

/// Role `R`'s answer to a request for clearance, if it refuses program messages.
fn clears<R, C>(ctx: &Ctx, config: &C) -> Result<Vec<SyncMessage>, Refusal>
where
    R: Policy<C>,
    C: HasSlot<R>,
{
    if !R::REFUSES_MESSAGES {
        return Ok(Vec::new());
    }
    sendable::<R>(ctx, R::may_run(ctx, config)?)
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
/// slot in the config, through [`HasSlot`], and its own [`Policy`]. The first role is the
/// root: its install creates the config, and the guard's own refusals name it.
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
}

impl<C, M, A> Roles<C, M> for (A,)
where
    A: Policy<C>,
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
        gate::<A, C, M>(ctx, config, message)
    }

    fn may_run(ctx: &Ctx, config: &C, message: &M) -> Result<Vec<SyncMessage>, Refusal> {
        may_run::<A, C, M>(ctx, config, message)
    }

    fn unchanged(after: &C, before: &C) -> Result<(), Refusal> {
        unchanged::<A, C>(after, before)
    }

    fn needs_clearance(message: &M) -> bool {
        asks::<A, C, M>(message)
    }

    fn clear(ctx: &Ctx, config: &C) -> Result<Vec<SyncMessage>, Refusal> {
        clears::<A, C>(ctx, config)
    }
}

impl<C, M, A, B> Roles<C, M> for (A, B)
where
    A: Policy<C>,
    B: Policy<C>,
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
        gate::<A, C, M>(ctx, config, message)?;
        gate::<B, C, M>(ctx, config, message)
    }

    fn may_run(ctx: &Ctx, config: &C, message: &M) -> Result<Vec<SyncMessage>, Refusal> {
        let mut sends = may_run::<A, C, M>(ctx, config, message)?;
        sends.extend(may_run::<B, C, M>(ctx, config, message)?);
        Ok(sends)
    }

    fn unchanged(after: &C, before: &C) -> Result<(), Refusal> {
        unchanged::<A, C>(after, before)?;
        unchanged::<B, C>(after, before)
    }
    fn needs_clearance(message: &M) -> bool {
        asks::<A, C, M>(message) || asks::<B, C, M>(message)
    }

    fn clear(ctx: &Ctx, config: &C) -> Result<Vec<SyncMessage>, Refusal> {
        let mut sends = clears::<A, C>(ctx, config)?;
        sends.extend(clears::<B, C>(ctx, config)?);
        Ok(sends)
    }
}
