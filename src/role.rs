//! What a role is, where its slot lives, its rules, and what those rules see.

use lee_core::account::AccountId;

use crate::*;

/// A role a slot plays, such as admin. Names the slot in panic text.
pub trait Role: 'static {
    /// The role's name, as it appears in a refusal's prefix and in a client's operation
    /// message. The guard knows a role by it, so a program's roles need a name each, and a
    /// guard over two roles of one name does not compile.
    const NAME: &'static str;
    /// The role's name in a refusal's sentence, such as "freeze authority" for the role named
    /// "freeze". By default its name.
    const PROSE_NAME: &'static str = Self::NAME;
    /// The type of the role's slot in the config, [`AuthoritySlot`] for a role that keeps
    /// only its holder.
    type Slot: Slot;
}

/// Where role `R`'s slot lives inside a config. The guard runs the management operations
/// on it and refuses a handler that changed it.
pub trait HasSlot<R: Role> {
    /// The slot.
    fn slot(&self) -> &R::Slot;
    /// The slot, to change it.
    fn slot_mut(&mut self) -> &mut R::Slot;
}

/// Who may install, nominate and renounce role `Self` in a config `C`, what the operations it
/// defines for itself do, and which program messages may run. Every method holds the rule of a
/// role that answers to nobody else, defines no operations of its own and lets every message
/// run, so such a role implements it empty. A role that answers to another, as a freeze
/// authority answers to the admin, overrides the rules that differ and reads the other role's
/// slot through `C`.
pub trait Policy<C>: Role + Sized
where
    C: HasSlot<Self>,
{
    /// Whether the role's holder is only ever appointed by nomination. Its `Init` is then
    /// refused (E137), and it cannot be a program's first role, whose `Init` creates the
    /// config. The guard attribute refuses a first role with it where `cargo check` sees it,
    /// at its `roles(..)`. A guard called by hand is refused at `cargo build`.
    const BY_NOMINATION_ONLY: bool = false;

    /// Whether the role's rules refuse program messages, as a freeze refuses every one while the
    /// program is frozen. The guard asks [`Policy::may_run`] only of a role that sets it, so rules
    /// a role does not declare here are never asked: at the config, and for a program message
    /// handled at another actor of the program, through a request for clearance to the config. A guard takes one such role at most, since a
    /// request for clearance names no role (ADR-0005), and two do not compile.
    const REFUSES_MESSAGES: bool = false;

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

    /// Runs one of the operations the role defines for itself, from the bytes of
    /// [`Op::Own`], and returns the role's slot as the operation leaves it. The guard checks no
    /// holder first, so the role's rules say who may send each operation. The guard writes back
    /// that slot alone and keeps its holder part as it was: the holder moves only through
    /// `Init`, `Nominate`, `Accept` and `Renounce`. By default the role defines none, and the
    /// operation is refused as a message that does not decode.
    ///
    /// # Errors
    /// The refusal that stops the operation.
    fn operate(_ctx: &Ctx, _config: &C, _op: &[u8]) -> Result<Self::Slot, Refusal> {
        Err(Refusal::of::<Self>(AuthorityError::MessageDoesNotDecode))
    }

    /// Whether a program message may run under the role's rules, as a freeze refuses every one
    /// while the program is frozen. The guard asks it of a role that sets
    /// [`Policy::REFUSES_MESSAGES`]: at the config, after the holders the message needs and
    /// before the handler, and for a message handled at another actor of the program, at its
    /// request for clearance after the handler. It is not asked for a message marked
    /// `#[exempt(Self)]`, and a management operation never reaches it, so the role's operations
    /// always run. By default every message may run.
    ///
    /// At the config, `ctx.caller()` is the sender of a message that came through its seat, and
    /// none for one sent straight to the config. At a request for clearance it is the sending
    /// actor's account, with no `via`, when that account authorized the transaction, whoever
    /// sent the message there, and none when it did not. So a credit at the actor of a
    /// recipient that signed the transaction has the recipient as its caller. [`Ctx::check`]
    /// refuses at a clearance, since the `via` is unknown there.
    ///
    /// # Errors
    /// The refusal that stops the message.
    fn may_run(_ctx: &Ctx, _config: &C) -> Result<(), Refusal> {
        Ok(())
    }
}

/// Who may install the first holder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitGate {
    /// The program's own account. On LEZ 0.4 that is the loader header account, whose key
    /// the deployer holds, so nobody else can win the race after deployment.
    Deployer,
    /// A key chosen at build time, for a program whose header account is not a key.
    Genesis(AccountId),
}

/// What a [`Policy`] sees: who is speaking, if the message came through the sender's own
/// actor, and the program's init gate.
pub struct Ctx {
    pub(crate) caller: Option<Caller>,
    pub(crate) program: AccountId,
    pub(crate) gate: InitGate,
}

impl Ctx {
    /// The sender, when the message came through the sender's own actor.
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
        slot.check(caller).map_err(Refusal::of::<R>)
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
            InitGate::Deployer => (self.program, AuthorityError::NotDeployer),
            InitGate::Genesis(key) => (key, AuthorityError::NotGenesis),
        };
        if caller.account != allowed || caller.via.is_some() {
            return Err(Refusal::of::<R>(refusal));
        }
        Ok(())
    }
}
