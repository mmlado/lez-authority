//! What a role adds to the guard beside its policy: its own operations and messages, its
//! consent to program messages and its looks at their state, with the rules the guard keeps
//! around each.

use lee_core::account::{AccountId, Actor};
use lee_core::program::{PdaSeed, Response, SyncMessage};
use lez_authority_core::{AuthorityError, HasSlot, InitGate, Refusal, Role, Slot};

use crate::policy::{Ctx, Policy};
use crate::wire::{self, Gated, LibraryMessage, Wire};

/// What role `Self` adds to the guard beside its [`Policy`]: the operations and messages it
/// defines for itself, and which program messages may run, over which state and with which
/// writes. Every item holds the hook of a role that defines no operations or messages of its own
/// and lets every message run and every write stand, so such a role implements it empty.
pub trait Hooks<C>: Policy<C>
where
    C: HasSlot<Self>,
{
    /// Whether the role's consent refuses program messages, as a freeze refuses every one while
    /// the program is frozen. The guard asks [`Hooks::may_run`] only of a role that sets it, so a
    /// consent a role does not declare here is never asked: at the config, and for a program
    /// message handled at another actor of the program, through a request for clearance to the
    /// config. A guard takes one such role at most, since a request for clearance names no role
    /// (lez-authority ADR-0005), and two do not compile.
    const REFUSES_MESSAGES: bool = false;

    /// Whether the role looks at the state of each actor the handler runs at, through
    /// [`Hooks::may_handle`] before the handler and [`Hooks::may_write`] after it. The guard
    /// asks only a role that sets it, so a program whose roles leave it unset pays nothing for
    /// the looks.
    const WATCHES_STATE: bool = false;

    /// Runs one of the operations the role defines for itself, from the bytes of
    /// [`Op::Own`](wire::Op::Own), and returns the role's slot as the operation leaves it, with
    /// the sync messages the config sends along with its new state. The guard checks no holder
    /// first, so the role's rules say who may send each operation. The guard writes back that
    /// slot alone and keeps its holder part as it was: the holder moves only through `Init`,
    /// `Nominate`, `Accept` and `Renounce`. It refuses the operation if one of the messages goes
    /// to an actor of the program (E139), since every actor of the program takes a message from
    /// the config as already gated, but for the role's own message, [`Ctx::own_message`], to an
    /// actor other than the config. By default the role defines none, and the operation is
    /// refused as a message that does not decode.
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
    /// [`Hooks::REFUSES_MESSAGES`]: at the config, after the holders the message needs and
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
    /// asks it of a role that sets [`Hooks::WATCHES_STATE`], just before the handler at every
    /// actor of the program, the config included. At the config that is after the holders the
    /// message needs and the consent. It asks for a message exempt from the role too. So a role
    /// can keep the program off any actor whose state it recognizes, as a freeze keeps it off its
    /// marks. Its refusal comes before a handler that cannot read that state fails on it.
    ///
    /// The look is not told which actor it runs at. So an actor that holds none of the role's
    /// state, such as a role's own actor before its first write, runs any handler. A management
    /// operation, a seat's forward, a request for clearance and a role's own message run no
    /// handler, so neither this look nor [`Hooks::may_write`] sees them. `ctx.caller()` is none.
    /// By default the handler runs at every actor.
    ///
    /// # Errors
    /// The refusal that stops the message, and the transaction with it.
    fn may_handle(_ctx: &Ctx, _state: &[u8]) -> Result<(), Refusal> {
        Ok(())
    }

    /// Whether the handler's write may stand, from the state of the actor it ran at, `before`,
    /// and the state it wrote there, `after`, or `None` when it kept the state. The guard asks it
    /// of a role that sets [`Hooks::WATCHES_STATE`] after the handler, wherever it asked
    /// [`Hooks::may_handle`], once the handler's sends are checked. So a role can keep the
    /// program from writing state it recognizes, as a freeze keeps it from writing a mark.
    /// `ctx.caller()` is none. By default every write stands.
    ///
    /// # Errors
    /// The refusal that stops the message, and the transaction with it.
    fn may_write(_ctx: &Ctx, _before: &[u8], _after: Option<&[u8]>) -> Result<(), Refusal> {
        Ok(())
    }
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

    /// Role `R`'s own message to its own actor for `key`, [`own_actor`], for `R`'s consent or
    /// operation to return. That actor hands `message` to [`Hooks::receive`]. The message names
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
}

/// The actor of role `R`'s own messages for `key`, in the program whose config actor is
/// `config`: a PDA of the program, under a seed hashed from a fixed tag, the config's account,
/// `key` and the role's name. Only `R`'s own messages for `key` run there, and a client lists it
/// in a transaction that reaches it.
#[must_use]
pub fn own_actor<R: Role>(config: Actor, key: &[u8; 32]) -> Actor {
    own_actor_and_seed(R::NAME, config, key).0
}

/// The actor of the own messages of the role named `role` for `key`, and the seed that derives it.
fn own_actor_and_seed(role: &str, config: Actor, key: &[u8; 32]) -> (Actor, PdaSeed) {
    use risc0_zkvm::sha::{Impl, Sha256 as _};
    const TAG: &[u8] = b"lez-authority/own-actor/v1";
    let mut bytes = Vec::with_capacity(TAG.len() + 64 + role.len());
    bytes.extend_from_slice(TAG);
    bytes.extend_from_slice(config.account_id.as_ref());
    bytes.extend_from_slice(key);
    // Last, so its length needs no prefix.
    bytes.extend_from_slice(role.as_bytes());
    let digest: [u8; 32] = Impl::hash_bytes(&bytes)
        .as_bytes()
        .try_into()
        .expect("a sha256 digest is 32 bytes");
    let seed = PdaSeed::new(digest);
    let program = config.program_account_id;
    let actor = Actor::new(AccountId::for_public_pda(&program, &seed), program);
    (actor, seed)
}

/// One of role `R`'s own operations, and the sync messages the config sends with it.
pub(crate) fn operate<R, C>(
    ctx: &Ctx,
    config: &mut C,
    op: &[u8],
) -> Result<Vec<SyncMessage>, Refusal>
where
    R: Hooks<C>,
    C: HasSlot<R>,
{
    let holder = HasSlot::<R>::slot(config).authority_slot().clone();
    let (mut slot, sends) = R::operate(ctx, config, op)?;
    let sends = sendable::<R>(ctx, sends)?;
    // The holder moves only through the holder operations, whatever the role returns.
    *slot.authority_slot_mut() = holder;
    *HasSlot::<R>::slot_mut(config) = slot;
    Ok(sends)
}

/// Role `R`'s sync messages, unless one goes to an actor of the program and is not `R`'s own
/// message to an actor other than the config.
fn sendable<R: Role>(ctx: &Ctx, sends: Vec<SyncMessage>) -> Result<Vec<SyncMessage>, Refusal> {
    // Every actor of the program takes the config's messages as gated, but hands a role's own to it.
    let refused = |sent: &SyncMessage| {
        sent.to.program_account_id == ctx.config.program_account_id
            && (sent.to == ctx.config
                || wire::own_message_role(&sent.message).as_deref() != Some(R::NAME))
    };
    if sends.iter().any(refused) {
        return Err(Refusal::of::<R>(AuthorityError::RoleSendsInProgram));
    }
    Ok(sends)
}

/// Role `R`'s consent to a program message at the config, unless the role declares no refusal or
/// the message is exempt from it.
pub(crate) fn may_run<R, C, M>(
    ctx: &Ctx,
    config: &C,
    message: &M,
) -> Result<Vec<SyncMessage>, Refusal>
where
    R: Hooks<C>,
    C: HasSlot<R>,
    M: Gated,
{
    if !R::REFUSES_MESSAGES || message.exempt::<R>() {
        return Ok(Vec::new());
    }
    sendable::<R>(ctx, R::may_run(ctx, config)?)
}

/// Whether role `R` refuses program messages and `message` is not exempt from it.
pub(crate) fn asks<R, C, M>(message: &M) -> bool
where
    R: Hooks<C>,
    C: HasSlot<R>,
    M: Gated,
{
    R::REFUSES_MESSAGES && !message.exempt::<R>()
}

/// Role `R`'s answer to a request for clearance, if it refuses program messages.
pub(crate) fn clears<R, C>(ctx: &Ctx, config: &C) -> Result<Vec<SyncMessage>, Refusal>
where
    R: Hooks<C>,
    C: HasSlot<R>,
{
    if !R::REFUSES_MESSAGES {
        return Ok(Vec::new());
    }
    sendable::<R>(ctx, R::may_run(ctx, config)?)
}

/// Role `R`'s own message for `key` at the actor `at`, which must be `R`'s own actor for `key`,
/// and that actor's new state.
pub(crate) fn receive<R, C>(
    ctx: &Ctx,
    key: &[u8; 32],
    at: Actor,
    state: &[u8],
    message: &[u8],
) -> Result<Option<Vec<u8>>, Refusal>
where
    R: Hooks<C>,
    C: HasSlot<R>,
{
    // A role's rules write only its own actors, never a seat or another role's.
    if own_actor::<R>(ctx.config, key) != at {
        return Err(Refusal::of::<R>(AuthorityError::NotOwnActor));
    }
    R::receive(ctx, state, message)
}

/// Role `R`'s look at an actor's state before the handler, if it watches state.
pub(crate) fn may_handle<R, C>(ctx: &Ctx, state: &[u8]) -> Result<(), Refusal>
where
    R: Hooks<C>,
    C: HasSlot<R>,
{
    if !R::WATCHES_STATE {
        return Ok(());
    }
    R::may_handle(ctx, state)
}

/// Role `R`'s look at the handler's write, if it watches state.
pub(crate) fn may_write<R, C>(ctx: &Ctx, before: &[u8], after: Option<&[u8]>) -> Result<(), Refusal>
where
    R: Hooks<C>,
    C: HasSlot<R>,
{
    if !R::WATCHES_STATE {
        return Ok(());
    }
    R::may_write(ctx, before, after)
}

/// Whether `response` sends a role's own message to an actor of `program`.
pub(crate) fn sends_own_message(response: &Response, program: AccountId) -> bool {
    wire::sent(response).any(|(to, message)| {
        to.program_account_id == program && wire::own_message_role(message).is_some()
    })
}
