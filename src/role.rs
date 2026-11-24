//! What a role is, where its slot lives, its rules, and what those rules see.

use lee_core::account::{AccountId, Actor};
use lee_core::program::SyncMessage;

use crate::routing::own_actor_and_seed;
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

/// Who may install, nominate and renounce role `Self` in a config `C`, what the operations and
/// messages it defines for itself do, and which program messages may run, over which state and
/// with which writes. Every method holds the rule of a role that answers to nobody else, defines
/// no operations or messages of its own and lets every message run and every write stand, so such
/// a role implements it empty. A role that answers to another, as a freeze authority answers to
/// the admin, overrides the rules that differ and reads the other role's slot through `C`.
pub trait Policy<C>: Role + Sized
where
    C: HasSlot<Self>,
{
    /// Whether the role's holder is only ever appointed by nomination. Its `Init` is then
    /// refused (E137), and it cannot be a program's first role, whose `Init` creates the
    /// config. The guard attribute refuses a first role with it where `cargo check` sees it,
    /// at its `roles(..)`. A guard called by hand is refused at `cargo build`.
    const BY_NOMINATION_ONLY: bool = false;

    /// Whether the role's consent refuses program messages, as a freeze refuses every one while
    /// the program is frozen. The guard asks [`Policy::may_run`] only of a role that sets it, so a
    /// consent a role does not declare here is never asked: at the config, and for a program message
    /// handled at another actor of the program, through a request for clearance to the config. A
    /// guard takes one such role at most, since a request for clearance names no role
    /// (lez-authority ADR-0005), and two do not compile.
    const REFUSES_MESSAGES: bool = false;

    /// Whether the role looks at the state of each actor the handler runs at, through
    /// [`Policy::may_handle`] before the handler and [`Policy::may_write`] after it. The guard
    /// asks only a role that sets it, so a program whose roles leave it unset pays nothing for
    /// the looks.
    const WATCHES_STATE: bool = false;

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
    /// [`Op::Own`], and returns the role's slot as the operation leaves it, with the sync
    /// messages the config sends along with its new state. The guard checks no holder first, so
    /// the role's rules say who may send each operation. The guard writes back that slot alone
    /// and keeps its holder part as it was: the holder moves only through `Init`, `Nominate`,
    /// `Accept` and `Renounce`. It refuses the operation if one of the messages goes to an actor
    /// of the program (E139), since every actor of the program takes a message from the config as
    /// already gated, but for the role's own message, [`Ctx::own_message`], to an actor other
    /// than the config. By default the role defines none, and the operation is refused as a
    /// message that does not decode.
    ///
    /// # Errors
    /// The refusal that stops the operation.
    fn operate(
        _ctx: &Ctx,
        _config: &C,
        _op: &[u8],
    ) -> Result<(Self::Slot, Vec<SyncMessage>), Refusal> {
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
    /// Its consent may come with sync messages, which the config sends ahead of the handler's
    /// own, or alone at a request for clearance. Each runs in the same transaction, so a refusal
    /// there refuses the message too. A message to an actor of the program among them is refused
    /// (E139), since every actor of the program takes a message from the config as already
    /// gated, but for the role's own message, [`Ctx::own_message`], to an actor other than the
    /// config.
    /// Like the handler's, the messages carry the grants the config's delivery came with.
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
    fn may_run(_ctx: &Ctx, _config: &C) -> Result<Vec<SyncMessage>, Refusal> {
        Ok(Vec::new())
    }

    /// Handles one of the role's own messages, [`Ctx::own_message`], at the role's own actor for
    /// the message's key, [`own_actor`], and returns that actor's new state, or `None` to keep
    /// it. `state` is the actor's state, and `ctx.caller()` is none. The guard takes the message
    /// only from the config and never at the config (E123), and only at that actor (E141), so a
    /// role writes no other actor's state. The handler never sees the message. By default the
    /// role defines none, and the message is refused as one that does not decode.
    ///
    /// # Errors
    /// The refusal that stops the message, and the transaction with it.
    fn receive(_ctx: &Ctx, _state: &[u8], _message: &[u8]) -> Result<Option<Vec<u8>>, Refusal> {
        Err(Refusal::of::<Self>(AuthorityError::MessageDoesNotDecode))
    }

    /// Whether the handler may run at an actor of the program, from that actor's state. The guard
    /// asks it of a role that sets [`Policy::WATCHES_STATE`], just before the handler at every
    /// actor of the program, the config included. At the config that is after the holders the
    /// message needs and the consent. It asks for a message exempt from the role too. So a role
    /// can keep the program off any actor whose state it recognizes, as a freeze keeps it off its
    /// marks. Its refusal comes before a handler that cannot read that state fails on it.
    ///
    /// The look is not told which actor it runs at. So an actor that holds none of the role's
    /// state, such as a role's own actor before its first write, runs any handler. A management
    /// operation, a seat's forward, a request for clearance and a role's own message run no
    /// handler, so neither this look nor [`Policy::may_write`] sees them. `ctx.caller()` is none.
    /// By default the handler runs at every actor.
    ///
    /// # Errors
    /// The refusal that stops the message, and the transaction with it.
    fn may_handle(_ctx: &Ctx, _state: &[u8]) -> Result<(), Refusal> {
        Ok(())
    }

    /// Whether the handler's write may stand, from the state of the actor it ran at, `before`,
    /// and the state it wrote there, `after`, or `None` when it kept the state. The guard asks it
    /// of a role that sets [`Policy::WATCHES_STATE`] after the handler, wherever it asked
    /// [`Policy::may_handle`], once the handler's sends are checked. So a role can keep the
    /// program from writing state it recognizes, as a freeze keeps it from writing a mark.
    /// `ctx.caller()` is none. By default every write stands.
    ///
    /// # Errors
    /// The refusal that stops the message, and the transaction with it.
    fn may_write(_ctx: &Ctx, _before: &[u8], _after: Option<&[u8]>) -> Result<(), Refusal> {
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
/// actor, the config's actor, and the program's init gate. At a role's own message and at the
/// looks around the handler it holds no caller.
pub struct Ctx {
    pub(crate) caller: Option<Caller>,
    pub(crate) config: Actor,
    pub(crate) gate: InitGate,
}

impl Ctx {
    /// A context with no caller.
    pub(crate) const fn without_caller(config: Actor, gate: InitGate) -> Self {
        Self {
            caller: None,
            config,
            gate,
        }
    }

    /// The config's actor, which also names the program.
    #[must_use]
    pub const fn config(&self) -> Actor {
        self.config
    }

    /// Role `R`'s own message to its own actor for `key`, [`own_actor`], for `R`'s consent or
    /// operation to return. That actor hands `message` to [`Policy::receive`]. The message names
    /// the actor's seed, so it reaches the actor before the actor holds any state.
    #[must_use]
    pub fn own_message<R: Role>(&self, key: [u8; 32], message: Vec<u8>) -> SyncMessage {
        let (to, seed) = own_actor_and_seed(R::NAME, self.config, &key);
        let own = Wire::<()>::Library(LibraryMessage::Own {
            role: R::NAME.into(),
            key,
            message,
        });
        SyncMessage::new(to, &own).with_pda_seeds([seed])
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
            InitGate::Deployer => (self.config.program_account_id, AuthorityError::NotDeployer),
            InitGate::Genesis(key) => (key, AuthorityError::NotGenesis),
        };
        if caller.account != allowed || caller.via.is_some() {
            return Err(Refusal::of::<R>(refusal));
        }
        Ok(())
    }
}
