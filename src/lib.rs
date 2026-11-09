//! Authority primitives for LEZ actor programs.
//!
//! A program keeps its config on one well-known account, the config actor, a PDA of the
//! program id and a seed. Inside that config sits an [`AuthoritySlot`] per role. A message
//! that needs a holder goes to the sender's own actor under the program first, the seat.
//! The seat checks `is_authorized` and forwards the message to the config, naming the program
//! that reached it. The config trusts the forward because only this program's own actors
//! reach it with `from_own_program()`, and every one of them runs the same code: [`guard()`].
//!
//! The program stays a plain LEZ program: its own message enum, its own state, and a handler
//! `fn(&ReceiveInput, Message) -> Response` under `lee_core::actor_entrypoint!`. The
//! [`guard`](macro@guard) attribute puts the guard in front of that handler. The library's
//! own messages, the roles' management operations and the hop, travel as a [`Wire`] message
//! behind a reserved first byte, so the program's enum hosts none of them. A program marks
//! the messages that need a holder, and the ones only the config may send, through
//! [`Gated`].
//!
//! A program names its roles as a tuple, [`Roles`]: one role, `(A,)`, or two, `(A, B)`.
//! Each keeps its own slot in the config. The first creates the config.
//!
//! This crate is policy-free. Which role a slot plays comes from the consumer through
//! [`Role`], and who may install, nominate and renounce through [`Policy`].

#![warn(missing_docs)]

// The macros name this crate by its path, which inside it needs this alias.
extern crate self as lez_authority;

/// Derive `HasSlot` from fields marked `#[slot(Role)]`, and `Gated` from messages marked
/// `#[only(Role)]` or `#[from_config]`. Put [`guard`](macro@guard) on the handler. See
/// [`lez_authority_macros`].
pub use lez_authority_macros::{Gated, HasSlot, guard};

use borsh::{BorshDeserialize, BorshSerialize};
use std::io::{Read, Write};

use lee_core::account::{AccountId, Actor};
use lee_core::program::{PdaSeed, ReceiveInput, Response};

/// Why a message was refused. It becomes a panic, which is how a handler fails on LEZ, with
/// a stable code in front of the sentence: `lez-authority/<role> E103: the signing account
/// is not the <role>`. A code never changes meaning. New refusals take new codes, retired
/// ones keep theirs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityError {
    /// The config account holds no state yet, and the message is not the root role's install.
    NotInitialized,
    /// The config account holds bytes that do not decode as the config.
    ConfigDoesNotDecode,
    /// A message does not encode or decode, or names a role the program does not have.
    MessageDoesNotDecode,
    /// The sender did not authorize the transaction at its own actor.
    MissingSignature,
    /// A forwarded message came from outside this program.
    NotOwnProgram,
    /// A forwarded message arrived somewhere other than the config account.
    ForwardedNotAtConfig,
    /// A message that needs a holder was sent straight to the config.
    NotThroughSeat,
    /// The slot holds nobody yet.
    NotInstalled,
    /// The slot already holds someone.
    AlreadyInstalled,
    /// Init came from an account other than the program's own, or not at top level.
    NotDeployer,
    /// Init came from an account other than the genesis key, or not at top level.
    NotGenesis,
    /// No nominee is pending.
    NoNomination,
    /// The caller is not the pending nominee.
    NotNominee,
    /// A nominee reached through a program did not prove itself with the seed that derives it.
    SeedProofFailed,
    /// The slot was renounced, for good.
    Renounced,
    /// The caller is not the holder.
    NotHolder,
    /// The caller is the holder's account but reached its actor through the wrong program.
    ViaNotAllowed,
    /// The program's handler changed a slot.
    SlotChanged,
    /// A message only the config sends came from somewhere else, or reached the config.
    NotFromConfig,
    /// The handler of a message that came through a seat sent to another program.
    ForeignSend,
}

impl AuthorityError {
    /// Every refusal, for the table test.
    pub const ALL: [Self; 20] = [
        Self::NotInitialized,
        Self::ConfigDoesNotDecode,
        Self::MessageDoesNotDecode,
        Self::MissingSignature,
        Self::NotOwnProgram,
        Self::ForwardedNotAtConfig,
        Self::NotThroughSeat,
        Self::NotInstalled,
        Self::AlreadyInstalled,
        Self::NotDeployer,
        Self::NotGenesis,
        Self::NoNomination,
        Self::NotNominee,
        Self::SeedProofFailed,
        Self::Renounced,
        Self::NotHolder,
        Self::ViaNotAllowed,
        Self::SlotChanged,
        Self::NotFromConfig,
        Self::ForeignSend,
    ];

    /// The stable code. Append-only: a number is never reused or renumbered.
    #[must_use]
    pub const fn code(&self) -> u16 {
        match self {
            Self::NotInitialized => 101,
            Self::Renounced => 102,
            Self::NotHolder => 103,
            Self::MissingSignature => 104,
            Self::SeedProofFailed => 107,
            Self::MessageDoesNotDecode => 108,
            Self::ConfigDoesNotDecode => 109,
            Self::NoNomination => 113,
            Self::NotNominee => 114,
            Self::NotGenesis => 115,
            Self::NotDeployer => 117,
            Self::AlreadyInstalled => 118,
            Self::NotOwnProgram => 119,
            Self::ViaNotAllowed => 120,
            Self::ForwardedNotAtConfig => 121,
            Self::NotThroughSeat => 122,
            Self::NotFromConfig => 123,
            Self::SlotChanged => 126,
            Self::NotInstalled => 131,
            Self::ForeignSend => 134,
        }
    }

    /// The panic text: the crate, the role, the code, the sentence.
    #[must_use]
    pub fn text(&self, role: &str) -> String {
        format!(
            "lez-authority/{role} E{}: {}",
            self.code(),
            self.describe(role)
        )
    }

    /// The sentence, naming the role.
    #[must_use]
    pub fn describe(&self, role: &str) -> String {
        match self {
            Self::NotInitialized => "the config account is not initialized".into(),
            Self::ConfigDoesNotDecode => "the config account does not decode".into(),
            Self::MessageDoesNotDecode => "the message does not decode".into(),
            Self::MissingSignature => "the sender must authorize the change".into(),
            Self::NotOwnProgram => {
                "a forwarded message only comes from this program's own actor".into()
            }
            Self::ForwardedNotAtConfig => {
                "forwarded messages are only handled at the config account".into()
            }
            Self::NotThroughSeat => {
                format!("a message that needs the {role} must go through the sender's own actor")
            }
            Self::NotInstalled => format!("no {role} is installed"),
            Self::AlreadyInstalled => format!("the {role} is already installed"),
            Self::NotDeployer => format!(
                "only the deployer installs the first {role}: the program's own account, through its seat at top level"
            ),
            Self::NotGenesis => {
                format!("only the genesis key installs the first {role}, through its seat at top level")
            }
            Self::NoNomination => format!("no {role} nominee is pending"),
            Self::NotNominee => format!("the signing account is not the nominated {role}"),
            Self::SeedProofFailed => {
                "a nominee reached through a program must prove itself with the seed that derives it under that program".into()
            }
            Self::Renounced => format!("the {role} is renounced"),
            Self::NotHolder => format!("the signing account is not the {role}"),
            Self::ViaNotAllowed => {
                format!("the {role} acts at top level, or through the program that made it the {role}")
            }
            Self::SlotChanged => format!("a handler must not change the {role} slot"),
            Self::NotFromConfig => "this message comes only from the config account".into(),
            Self::ForeignSend => {
                "a message that came through a seat sends only to actors of this program".into()
            }
        }
    }
}

/// A refusal as a value: what was refused, and the role whose rule refused it. [`guard()`]
/// panics with its [`text`](Self::text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The role named in the text.
    pub role: &'static str,
    /// What was refused.
    pub error: AuthorityError,
}

impl Refusal {
    /// `error`, refused by role `R`'s rule.
    #[must_use]
    pub fn of<R: Role>(error: AuthorityError) -> Self {
        Self {
            role: R::NAME,
            error,
        }
    }

    /// The panic text.
    #[must_use]
    pub fn text(&self) -> String {
        self.error.text(self.role)
    }
}

/// Who holds a role, and through which program they act. `via` is `None` for a key that
/// signs at top level, and the owning program for a PDA that is granted by it.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Holder {
    /// The holder's account.
    pub account: AccountId,
    /// The program the holder acts through, or `None` for top level.
    pub via: Option<AccountId>,
}

/// The sender of a forwarded message, as the guard saw it: the account whose own actor
/// vouched for it, and the program that reached that actor. Only the guard builds one.
#[derive(Clone, Copy, Debug)]
pub struct Caller {
    account: AccountId,
    via: Option<AccountId>,
}

impl Caller {
    /// The account that authorized the message at its own actor.
    #[must_use]
    pub fn account(&self) -> AccountId {
        self.account
    }

    /// The program that reached the sender's actor, or `None` at top level.
    #[must_use]
    pub fn via(&self) -> Option<AccountId> {
        self.via
    }
}

/// One role's slot: nobody yet, a holder with at most one pending nominee, or renounced
/// for good.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default, PartialEq, Eq)]
pub enum AuthoritySlot {
    /// No holder yet. The state of a config nobody initialized.
    #[default]
    Unset,
    /// Held, with the holder's standing offer of the role to `nominee` if any.
    Held {
        /// The holder.
        holder: Holder,
        /// The account the holder nominated, until it accepts or the offer changes.
        nominee: Option<AccountId>,
    },
    /// Renounced. Terminal.
    Renounced,
}

impl AuthoritySlot {
    /// A slot held by `holder` with no nominee.
    #[must_use]
    pub fn held(holder: Holder) -> Self {
        Self::Held {
            holder,
            nominee: None,
        }
    }

    /// The holder, or `None` when unset or renounced.
    #[must_use]
    pub fn holder(&self) -> Option<Holder> {
        match self {
            Self::Held { holder, .. } => Some(*holder),
            Self::Unset | Self::Renounced => None,
        }
    }

    /// The pending nominee, if any.
    #[must_use]
    pub fn nominee(&self) -> Option<AccountId> {
        match self {
            Self::Held { nominee, .. } => *nominee,
            Self::Unset | Self::Renounced => None,
        }
    }

    /// Whether `caller` is the holder, reached the way the holder acts.
    ///
    /// # Errors
    /// `NotInstalled`, `Renounced`, `NotHolder` for another account, `ViaNotAllowed` for
    /// the holder's account reached through the wrong program.
    pub fn check(&self, caller: &Caller) -> Result<(), AuthorityError> {
        let holder = match self {
            Self::Unset => return Err(AuthorityError::NotInstalled),
            Self::Renounced => return Err(AuthorityError::Renounced),
            Self::Held { holder, .. } => holder,
        };
        if holder.account != caller.account {
            return Err(AuthorityError::NotHolder);
        }
        if holder.via != caller.via {
            return Err(AuthorityError::ViaNotAllowed);
        }
        Ok(())
    }
}

/// A role a slot plays, such as admin. Names the slot in panic text.
pub trait Role: 'static {
    /// The role's name, as it appears in panic text and in a client's operation message.
    const NAME: &'static str;
}

/// Where role `R`'s slot lives inside a config. The guard runs the management operations
/// on it and refuses a handler that changed it.
pub trait HasSlot<R: Role> {
    /// The slot.
    fn slot(&self) -> &AuthoritySlot;
    /// The slot, to change it.
    fn slot_mut(&mut self) -> &mut AuthoritySlot;
}

/// Who may install, nominate and renounce role `Self` in a config `C`. Every method holds
/// the rule of a role that answers to nobody else, so such a role implements it empty. A
/// role that answers to another, as a freeze authority answers to the admin, overrides the
/// rules that differ and reads the other role's slot through `C`.
pub trait Policy<C>: Role + Sized
where
    C: HasSlot<Self>,
{
    /// Who may install the first holder of the unset slot. By default whoever passes the
    /// program's init gate, at top level.
    ///
    /// # Errors
    /// The refusal that stops the install.
    fn may_install(ctx: &Ctx, _config: &C) -> Result<(), Refusal> {
        ctx.passes_gate::<Self>()
    }

    /// Who may nominate. By default the holder.
    ///
    /// # Errors
    /// The refusal that stops the nomination.
    fn may_nominate(ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        ctx.check::<Self>(HasSlot::<Self>::slot(config))
    }

    /// Who may renounce. By default the holder.
    ///
    /// # Errors
    /// The refusal that stops the renounce.
    fn may_renounce(ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        ctx.check::<Self>(HasSlot::<Self>::slot(config))
    }

    /// What renounce leaves in the slot. By default renounced, for good.
    #[must_use]
    fn renounced() -> AuthoritySlot {
        AuthoritySlot::Renounced
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

/// A management operation on a role's slot. A client sends it as a library message, the
/// guard runs it under the role's [`Policy`], and the handler never sees it.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Op {
    /// Makes the sender the holder of an unset slot.
    Init,
    /// The holder offers the role to `next`. The all-zero id cancels, a later offer replaces
    /// an earlier one. Nothing moves until `next` accepts.
    Nominate {
        /// The account offered the role.
        next: AccountId,
    },
    /// The pending nominee takes the role, through its own seat. A key accepts at top level.
    /// A PDA accepts through the program that derives it, and carries that seed as proof.
    Accept {
        /// The seed that derives the nominee under the program that reached its seat.
        pda_seed: Option<PdaSeed>,
    },
    /// Gives the role up, nominee included. By default the sender must be the holder, and
    /// the slot stays empty for good.
    Renounce,
}

/// A message the sender's own actor forwarded to the config, naming the program that
/// reached it. Clients never send it: the config accepts it only from an actor of the same
/// program.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct Forwarded {
    /// The program that reached the sender's actor, or `None` at top level.
    pub via: Option<AccountId>,
    /// The original message, as it arrived at the seat.
    pub inner: Vec<u8>,
}

/// The library's own messages, which never reach the program's handler.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub enum LibraryMessage {
    /// A management operation on the slot of the role with this name.
    Op {
        /// The role's [`Role::NAME`].
        role: String,
        /// The operation.
        op: Op,
    },
    /// The hop from a seat to the config.
    Forwarded(Forwarded),
}

/// The first byte of a library message. A program's message enum must stay below 255
/// variants, so that none of its messages starts with it.
pub const LIBRARY_TAG: u8 = 255;

/// What arrives at a guarded handler: one of the program's own messages, encoded as the
/// program encodes it, or a library message behind [`LIBRARY_TAG`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wire<M> {
    /// One of the program's messages.
    Program(M),
    /// One of the library's.
    Library(LibraryMessage),
}

impl<M> Wire<M> {
    /// A client's message for one of role `R`'s management operations.
    #[must_use]
    pub fn op<R: Role>(op: Op) -> Self {
        Self::Library(LibraryMessage::Op {
            role: R::NAME.into(),
            op,
        })
    }
}

impl<M> From<M> for Wire<M> {
    fn from(message: M) -> Self {
        Self::Program(message)
    }
}

impl<M: BorshSerialize> BorshSerialize for Wire<M> {
    fn serialize<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        match self {
            Self::Program(message) => message.serialize(writer),
            Self::Library(message) => {
                writer.write_all(&[LIBRARY_TAG])?;
                message.serialize(writer)
            }
        }
    }
}

impl<M: BorshDeserialize> BorshDeserialize for Wire<M> {
    fn deserialize_reader<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        let mut first = [0u8; 1];
        reader.read_exact(&mut first)?;
        if first[0] == LIBRARY_TAG {
            LibraryMessage::deserialize_reader(reader).map(Self::Library)
        } else {
            M::deserialize_reader(&mut (&first[..]).chain(reader)).map(Self::Program)
        }
    }
}

/// What the program's messages declare to the guard. Derive it with `#[only(Role)]` on a
/// message that needs a role's holder, the way `onlyOwner` marks a Solidity function, and
/// `#[from_config]` on one that only the config account may send.
pub trait Gated {
    /// Whether this message needs role `R`'s holder. The guard then takes it only through
    /// the sender's own actor and checks the holder before the handler runs.
    fn needs<R: Role>(&self) -> bool {
        false
    }

    /// Whether only the config account may send this message, as it does to the program's
    /// other actors.
    fn only_from_config(&self) -> bool {
        false
    }
}

/// What a [`Policy`] sees: who is speaking, if the message came through the sender's own
/// actor, and the program's init gate.
pub struct Ctx {
    caller: Option<Caller>,
    program: AccountId,
    gate: InitGate,
}

impl Ctx {
    /// The sender, when the message came through the sender's own actor.
    #[must_use]
    pub fn caller(&self) -> Option<&Caller> {
        self.caller.as_ref()
    }

    /// Whether the message came through the sender's own actor and the sender holds role
    /// `R` in `slot`.
    ///
    /// # Errors
    /// `NotThroughSeat`, or the slot's refusal of the sender, naming `R`.
    pub fn check<R: Role>(&self, slot: &AuthoritySlot) -> Result<(), Refusal> {
        slot.check(self.vouched::<R>()?).map_err(Refusal::of::<R>)
    }

    fn vouched<R: Role>(&self) -> Result<&Caller, Refusal> {
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

/// One management operation on `R`'s slot, under `R`'s policy.
fn run_op<R, C>(ctx: &Ctx, config: &mut C, op: Op) -> Result<(), Refusal>
where
    R: Policy<C>,
    C: HasSlot<R>,
{
    let refuse = Refusal::of::<R>;
    match op {
        Op::Init => {
            R::may_install(ctx, config)?;
            let caller = *ctx.vouched::<R>()?;
            let slot = HasSlot::<R>::slot_mut(config);
            match slot {
                AuthoritySlot::Unset => {}
                AuthoritySlot::Held { .. } => return Err(refuse(AuthorityError::AlreadyInstalled)),
                AuthoritySlot::Renounced => return Err(refuse(AuthorityError::Renounced)),
            }
            *slot = AuthoritySlot::held(Holder {
                account: caller.account,
                via: caller.via,
            });
        }
        Op::Nominate { next } => {
            R::may_nominate(ctx, config)?;
            match HasSlot::<R>::slot_mut(config) {
                AuthoritySlot::Held { nominee, .. } => {
                    *nominee = (next != AccountId::default()).then_some(next);
                }
                AuthoritySlot::Unset => return Err(refuse(AuthorityError::NotInstalled)),
                AuthoritySlot::Renounced => return Err(refuse(AuthorityError::Renounced)),
            }
        }
        Op::Accept { pda_seed } => {
            let caller = *ctx.vouched::<R>()?;
            let slot = HasSlot::<R>::slot_mut(config);
            let pending = match slot {
                AuthoritySlot::Unset => return Err(refuse(AuthorityError::NotInstalled)),
                AuthoritySlot::Renounced => return Err(refuse(AuthorityError::Renounced)),
                AuthoritySlot::Held { nominee, .. } => *nominee,
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
            *HasSlot::<R>::slot_mut(config) = R::renounced();
        }
    }
    Ok(())
}

/// The holder a message declares it needs, checked before the handler.
fn gate<R, C, M>(ctx: &Ctx, config: &C, message: &M) -> Result<(), Refusal>
where
    R: Role,
    C: HasSlot<R>,
    M: Gated,
{
    if message.needs::<R>() {
        ctx.check::<R>(HasSlot::<R>::slot(config))?;
    }
    Ok(())
}

/// The handler must leave every slot as it found it.
fn unchanged<R, C>(config: &C, before: &AuthoritySlot) -> Result<(), Refusal>
where
    R: Role,
    C: HasSlot<R>,
{
    if HasSlot::<R>::slot(config) == before {
        Ok(())
    } else {
        Err(Refusal::of::<R>(AuthorityError::SlotChanged))
    }
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
    /// The place in the tuple of the role with this name.
    #[doc(hidden)]
    fn index(name: &str) -> Option<usize>;
    /// Runs an operation of the role at `role`.
    #[doc(hidden)]
    fn run_op(ctx: &Ctx, config: &mut C, role: usize, op: Op) -> Result<(), Refusal>;
    /// Whether the message needs any role's holder.
    #[doc(hidden)]
    fn needs_any(message: &M) -> bool;
    /// Checks the holders the message declares it needs.
    #[doc(hidden)]
    fn gate(ctx: &Ctx, config: &C, message: &M) -> Result<(), Refusal>;
    /// Every role's slot, in order.
    #[doc(hidden)]
    fn slots(config: &C) -> Vec<AuthoritySlot>;
    /// Refuses a slot that differs from `before`.
    #[doc(hidden)]
    fn unchanged(config: &C, before: &[AuthoritySlot]) -> Result<(), Refusal>;
}

impl<C, M, A> Roles<C, M> for (A,)
where
    A: Policy<C>,
    C: HasSlot<A>,
    M: Gated,
{
    const ROOT: &'static str = A::NAME;

    fn index(name: &str) -> Option<usize> {
        (name == A::NAME).then_some(0)
    }

    fn run_op(ctx: &Ctx, config: &mut C, _role: usize, op: Op) -> Result<(), Refusal> {
        run_op::<A, C>(ctx, config, op)
    }

    fn needs_any(message: &M) -> bool {
        message.needs::<A>()
    }

    fn gate(ctx: &Ctx, config: &C, message: &M) -> Result<(), Refusal> {
        gate::<A, C, M>(ctx, config, message)
    }

    fn slots(config: &C) -> Vec<AuthoritySlot> {
        vec![HasSlot::<A>::slot(config).clone()]
    }

    fn unchanged(config: &C, before: &[AuthoritySlot]) -> Result<(), Refusal> {
        unchanged::<A, C>(config, &before[0])
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

    fn index(name: &str) -> Option<usize> {
        if name == A::NAME {
            Some(0)
        } else {
            (name == B::NAME).then_some(1)
        }
    }

    fn run_op(ctx: &Ctx, config: &mut C, role: usize, op: Op) -> Result<(), Refusal> {
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

    fn slots(config: &C) -> Vec<AuthoritySlot> {
        vec![
            HasSlot::<A>::slot(config).clone(),
            HasSlot::<B>::slot(config).clone(),
        ]
    }

    fn unchanged(config: &C, before: &[AuthoritySlot]) -> Result<(), Refusal> {
        unchanged::<A, C>(config, &before[0])?;
        unchanged::<B, C>(config, &before[1])
    }
}

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

/// Runs `handler` behind the guard. The [`guard`](macro@guard) attribute writes this call,
/// and a program may write it itself. `Rs` is the program's roles, `(A,)` or `(A, B)`, `C`
/// the config the program keeps on the config account, and `gate` says who may install the
/// root role.
///
/// - A library operation goes to the config through the sender's seat, and the guard runs it
///   there under the role's policy. The root role's `Init` creates the config.
/// - A message that needs a holder goes the same way. At the config the guard checks the
///   holder, runs the handler, and refuses a result that changed a slot or sent to another
///   program.
/// - A message only the config sends runs only when it came from the config account.
/// - Anything else runs where it was sent, as in a program without the guard. At the config
///   that needs an initialized config, and the slots are checked afterwards.
///
/// The handler of a message that came through a seat sees `input.from` as the sender's own
/// actor and `input.message` as the hop.
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
    let refuse = |error| Refusal {
        role: Rs::ROOT,
        error,
    };
    let program = input.receiver.program_account_id;
    let config_actor = Actor::new(config_account(program, seed), program);
    let at_config = input.receiver == config_actor;
    let from_config = input.from == Some(config_actor);
    match message {
        Wire::Program(message) => {
            if message.only_from_config() {
                if at_config || !from_config {
                    return Err(refuse(AuthorityError::NotFromConfig));
                }
                return Ok(handler(input, message));
            }
            if !at_config {
                if from_config || !Rs::needs_any(&message) {
                    return Ok(handler(input, message));
                }
                return seat(input, config_actor).map_err(refuse);
            }
            at_the_config::<Rs, C, M>(input, program, gate, None, message, handler)
        }
        Wire::Library(LibraryMessage::Op { .. }) => {
            if at_config {
                return Err(refuse(AuthorityError::NotThroughSeat));
            }
            seat(input, config_actor).map_err(refuse)
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
            };
            let inner: Wire<M> = borsh::from_slice(&hop.inner)
                .map_err(|_| refuse(AuthorityError::MessageDoesNotDecode))?;
            match inner {
                Wire::Library(LibraryMessage::Op { role, op }) => {
                    let role = Rs::index(&role)
                        .ok_or_else(|| refuse(AuthorityError::MessageDoesNotDecode))?;
                    manage::<Rs, C, M>(input, program, gate, caller, role, op)
                }
                Wire::Library(LibraryMessage::Forwarded(_)) => {
                    Err(refuse(AuthorityError::MessageDoesNotDecode))
                }
                Wire::Program(message) => {
                    if message.only_from_config() {
                        return Err(refuse(AuthorityError::NotFromConfig));
                    }
                    at_the_config::<Rs, C, M>(input, program, gate, Some(caller), message, handler)
                }
            }
        }
    }
}

/// A management operation at the config, which the root role's `Init` may create.
fn manage<Rs, C, M>(
    input: &ReceiveInput,
    program: AccountId,
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
        program,
        gate,
    };
    Rs::run_op(&ctx, &mut config, role, op)?;
    let bytes = borsh::to_vec(&config).map_err(|_| refuse(AuthorityError::ConfigDoesNotDecode))?;
    Ok(Response::set_state(bytes))
}

/// A program message at the config: the declared holders, the handler, then the slots and,
/// for a vouched message, where the handler sent.
fn at_the_config<Rs, C, M>(
    input: &ReceiveInput,
    program: AccountId,
    gate: InitGate,
    caller: Option<Caller>,
    message: M,
    handler: impl FnOnce(&ReceiveInput, M) -> Response,
) -> Result<Response, Refusal>
where
    Rs: Roles<C, M>,
    C: BorshSerialize + BorshDeserialize + Default,
    M: Gated,
{
    let refuse = |error| Refusal {
        role: Rs::ROOT,
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
        program,
        gate,
    };
    Rs::gate(&ctx, &config, &message)?;
    let before = Rs::slots(&config);
    let response = handler(input, message);
    if let Some(post) = &response.post_state {
        let post: &[u8] = post;
        let after: C =
            borsh::from_slice(post).map_err(|_| refuse(AuthorityError::ConfigDoesNotDecode))?;
        Rs::unchanged(&after, &before)?;
    }
    // A PDA holder's grant travels with the call, so a vouched message stays in the program.
    if caller.is_some() {
        let sync = response.sync_messages.iter().map(|sent| sent.to);
        let not_ours = sync
            .chain(response.async_messages.iter().map(|sent| sent.to))
            .any(|to| to.program_account_id != program);
        if not_ours {
            return Err(refuse(AuthorityError::ForeignSend));
        }
    }
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

#[cfg(test)]
mod tests;
