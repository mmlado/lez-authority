//! Who may hold a role: the rules a role sets on its holder, what they see, and the holder
//! operations and checks under them.

use lee_core::account::{AccountId, Actor};
use lee_core::program::PdaSeed;
use lez_authority_core::{
    AuthorityError, AuthoritySlot, HasSlot, Holder, InitGate, Refusal, Role, Slot,
};

use crate::wire::Gated;

/// Who may install, nominate and renounce role `Self` in a config `C`, and what renounce leaves.
/// Every method holds the rule of a role that answers to nobody else, so such a role implements
/// it empty. A role that answers to another, as a freeze authority answers to the admin,
/// overrides the rules that differ and reads the other role's slot through `C`.
pub trait Policy<C>: Role + Sized
where
    C: HasSlot<Self>,
{
    /// Whether the role's holder is only ever appointed by nomination. Its `Init` is then
    /// refused (E137), and it cannot be a program's first role, whose `Init` creates the
    /// config. The guard attribute refuses a first role with it where `cargo check` sees it,
    /// at its `roles(..)`. A guard called by hand is refused at `cargo build`.
    const BY_NOMINATION_ONLY: bool = false;

    /// Who may install the first holder of a slot that has none. By default whoever passes the
    /// program's init gate, at top level.
    ///
    /// # Errors
    /// The refusal that stops the install.
    fn may_install(ctx: &Ctx, _config: &C) -> Result<(), Refusal> {
        ctx.passes_gate::<Self>()
    }

    /// Who may nominate. By default the holder. Where a policy lets someone else nominate on
    /// a slot with no holder, the slot gets a standing offer.
    ///
    /// # Errors
    /// The refusal that stops the nomination.
    fn may_nominate(ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        ctx.check::<Self>(HasSlot::<Self>::slot(config).authority_slot())
    }

    /// Who may renounce. By default the holder.
    ///
    /// # Errors
    /// The refusal that stops the renounce.
    fn may_renounce(ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        ctx.check::<Self>(HasSlot::<Self>::slot(config).authority_slot())
    }

    /// What renounce leaves in the slot's holder part. By default renounced, for good.
    #[must_use]
    fn renounced() -> AuthoritySlot {
        AuthoritySlot::Renounced
    }
}

/// What a role's [`Policy`] and hooks see: who is speaking, if the message came through the
/// sender's own actor, the config's actor, and the program's init gate. At a role's own message
/// and at the looks around the handler it holds no caller.
pub struct Ctx {
    pub(crate) caller: Option<Caller>,
    pub(crate) config: Actor,
    pub(crate) gate: InitGate,
}

impl Ctx {
    /// The config's actor, which also names the program.
    #[must_use]
    pub const fn config(&self) -> Actor {
        self.config
    }

    /// The sender, when the message came through the sender's own actor. None at a role's own
    /// message and at the looks around the handler.
    #[must_use]
    pub fn caller(&self) -> Option<&Caller> {
        self.caller.as_ref()
    }

    /// Whether the message came through the sender's own actor and the sender holds role
    /// `R` in `slot`. At a request for clearance it refuses, since the program that reached
    /// the sender's actor is unknown there.
    ///
    /// # Errors
    /// `NotThroughSeat`, or the slot's refusal of the sender, naming `R`.
    pub fn check<R: Role>(&self, slot: &AuthoritySlot) -> Result<(), Refusal> {
        let caller = self.vouched::<R>()?;
        // A request for clearance carries no via, so the holder's via cannot be compared.
        if !caller.via_known {
            return Err(Refusal::of::<R>(AuthorityError::NotThroughSeat));
        }
        slot.check(&caller.account, &caller.via)
            .map_err(Refusal::of::<R>)
    }

    pub(crate) fn vouched<R: Role>(&self) -> Result<&Caller, Refusal> {
        self.caller
            .as_ref()
            .ok_or_else(|| Refusal::of::<R>(AuthorityError::NotThroughSeat))
    }

    /// The init gate: the program's own account, or the genesis key, at top level.
    fn passes_gate<R: Role>(&self) -> Result<(), Refusal> {
        let caller = self.vouched::<R>()?;
        let (allowed, refusal) = match self.gate {
            InitGate::Deployer => (self.config.program_account_id, AuthorityError::NotDeployer),
            InitGate::Genesis(key) => (key, AuthorityError::NotGenesis),
        };
        if caller.account != allowed || caller.via.is_some() {
            return Err(Refusal::of::<R>(refusal));
        }
        Ok(())
    }
}

/// The sender as the guard saw it: the account whose own actor vouched for a forwarded message
/// and the program that reached that actor, or at a request for clearance the account alone,
/// since the request does not carry that program. Only the guard builds one.
#[derive(Clone, Copy, Debug)]
pub struct Caller {
    pub(crate) account: AccountId,
    pub(crate) via: Option<AccountId>,
    pub(crate) via_known: bool,
}

impl Caller {
    /// The account that authorized the message at its own actor.
    #[must_use]
    pub fn account(&self) -> AccountId {
        self.account
    }

    /// The program that reached the sender's actor, or `None` at top level. At a request for
    /// clearance it is `None` too, whatever program reached the actor.
    #[must_use]
    pub fn via(&self) -> Option<AccountId> {
        self.via
    }
}

/// Makes the sender the first holder of `R`'s slot, under `R`'s policy.
pub(crate) fn init<R, C>(ctx: &Ctx, config: &mut C) -> Result<(), Refusal>
where
    R: Policy<C>,
    C: HasSlot<R>,
{
    if R::BY_NOMINATION_ONLY {
        return Err(Refusal::of::<R>(AuthorityError::ByNominationOnly));
    }
    R::may_install(ctx, config)?;
    let caller = *ctx.vouched::<R>()?;
    HasSlot::<R>::slot_mut(config)
        .authority_slot_mut()
        .install(Holder {
            account: caller.account,
            via: caller.via,
        })
        .map_err(Refusal::of::<R>)
}

/// Offers `R`'s slot to `next` under `R`'s policy, or withdraws the offer for the all-zero id.
pub(crate) fn nominate<R, C>(ctx: &Ctx, config: &mut C, next: AccountId) -> Result<(), Refusal>
where
    R: Policy<C>,
    C: HasSlot<R>,
{
    R::may_nominate(ctx, config)?;
    let offer = (next != AccountId::default()).then_some(next);
    HasSlot::<R>::slot_mut(config)
        .authority_slot_mut()
        .nominate(offer)
        .map_err(Refusal::of::<R>)
}

/// Makes the sender, `R`'s pending nominee, its holder. A PDA nominee proves itself with the
/// seed that derives it.
pub(crate) fn accept<R, C>(
    ctx: &Ctx,
    config: &mut C,
    pda_seed: Option<PdaSeed>,
) -> Result<(), Refusal>
where
    R: Role,
    C: HasSlot<R>,
{
    let caller = *ctx.vouched::<R>()?;
    let by = Holder {
        account: caller.account,
        via: caller.via,
    };
    HasSlot::<R>::slot_mut(config)
        .authority_slot_mut()
        .accept(by, pda_seed)
        .map_err(Refusal::of::<R>)
}

/// Gives up `R`'s slot under `R`'s policy, leaving what the policy says.
pub(crate) fn renounce<R, C>(ctx: &Ctx, config: &mut C) -> Result<(), Refusal>
where
    R: Policy<C>,
    C: HasSlot<R>,
{
    R::may_renounce(ctx, config)?;
    HasSlot::<R>::slot_mut(config)
        .authority_slot_mut()
        .renounce(R::renounced());
    Ok(())
}

/// The holder a message declares it needs, checked before the handler.
pub(crate) fn gate<R, C, M>(ctx: &Ctx, config: &C, message: &M) -> Result<(), Refusal>
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
