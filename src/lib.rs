//! Authority primitives for LEZ actor programs.
//!
//! A program keeps its config on one well-known account, the config actor, a PDA of the
//! program id and a seed. Inside that config sits an [`AuthoritySlot`] per role. A message
//! that needs the holder's authority goes to the sender's own actor under the program
//! first. That actor checks `is_authorized` and forwards the message to the config, naming
//! the program that reached it. The config trusts the forward because only this program's
//! own actors reach it with `from_own_program()`, and every one of them runs the same
//! code: [`guard`].
//!
//! The program keeps its own message enum and its own handler. The enum hosts two library
//! variants through [`Hosts`]: the management operations [`Op`], which the guard runs
//! without the handler, and [`Forwarded`], the hop. The handler receives a [`Ctx`] and
//! calls it where a message needs the holder. The guard then checks that the slot changed
//! only through the context.
//!
//! This crate is policy-free. Which role a slot plays comes from the consumer through
//! [`Role`].

#![warn(missing_docs)]

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, Actor};
use lee_core::program::{PdaSeed, ReceiveInput, Response};

/// Why a message was refused. It becomes a panic, which is how a handler fails on LEZ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityError {
    /// The config account holds no state yet, and the message is not the install operation.
    NotInitialized,
    /// The config account holds bytes that do not decode as the config.
    ConfigDoesNotDecode,
    /// A message does not encode or decode.
    MessageDoesNotDecode,
    /// The sender did not authorize the transaction at its own actor.
    MissingSignature,
    /// A forwarded message came from outside this program.
    NotOwnProgram,
    /// A forwarded message arrived somewhere other than the config account.
    ForwardedNotAtConfig,
    /// A message that needs the holder was sent straight to the config.
    NotThroughSeat,
    /// The slot holds nobody yet.
    NotInstalled,
    /// The slot already holds someone.
    AlreadyInstalled,
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
    /// The program's handler changed the slot by other means than the context.
    SlotChanged,
}

impl AuthorityError {
    /// The panic text, naming the role.
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
            Self::SlotChanged => {
                format!("a handler must not change the {role} slot except through the context")
            }
        }
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

/// The sender of a forwarded message, as the engine saw it: the account whose own actor
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
    /// The role's name, as it appears in panic text.
    const NAME: &'static str;
}

/// Where role `R`'s slot lives inside a config. The guard runs the management operations
/// on it and checks that the handler changed it only through the context.
pub trait HasSlot<R: Role> {
    /// The slot.
    fn slot(&self) -> &AuthoritySlot;
    /// The slot, to change it.
    fn slot_mut(&mut self) -> &mut AuthoritySlot;
}

/// A management operation on the slot. The program hosts it in its message enum, the
/// guard runs it, and the handler never sees it.
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
    /// Empties the slot for good, nominee included. The sender must be the holder.
    Renounce,
}

/// A message the sender's own actor forwarded to the config, naming the program that
/// reached it. The program hosts it in its message enum. Clients never send it: the
/// config accepts it only from an actor of the same program.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct Forwarded {
    /// The program that reached the sender's actor, or `None` at top level.
    pub via: Option<AccountId>,
    /// The encoded original message.
    pub inner: Vec<u8>,
}

/// A message enum that hosts the library's two variants, [`Op`] and [`Forwarded`].
pub trait Hosts: BorshSerialize + BorshDeserialize {
    /// Wraps a hop in its hosting variant.
    fn forwarded(hop: Forwarded) -> Self;
    /// Unwraps the hop, or gives the message back.
    ///
    /// # Errors
    /// The message itself when it is not the hop.
    fn into_forwarded(self) -> Result<Forwarded, Self>;
    /// Unwraps a management operation, or gives the message back.
    ///
    /// # Errors
    /// The message itself when it is not an operation.
    fn into_op(self) -> Result<Op, Self>;
}

/// What the guard hands the handler: who is speaking, if the message came through the
/// sender's own actor, and the operations on the slot. Every method panics on refusal,
/// which is how a handler fails on LEZ.
pub struct Ctx {
    caller: Option<Caller>,
    role: &'static str,
    expected: Option<AuthoritySlot>,
}

impl Ctx {
    /// The sender, when the message came through the sender's own actor.
    #[must_use]
    pub fn caller(&self) -> Option<&Caller> {
        self.caller.as_ref()
    }

    /// Panics unless the message came through the sender's own actor and the sender is
    /// the holder.
    pub fn require(&self, slot: &AuthoritySlot) {
        let caller = self.vouched();
        if let Err(error) = slot.check(caller) {
            self.fail(&error);
        }
    }

    /// Installs the sender as the holder of an unset slot. The hosted [`Op::Init`] does
    /// this without the handler; call it yourself from an init message of your own.
    pub fn install(&mut self, slot: &mut AuthoritySlot) {
        let caller = *self.vouched();
        match slot {
            AuthoritySlot::Unset => {}
            AuthoritySlot::Held { .. } => self.fail(&AuthorityError::AlreadyInstalled),
            AuthoritySlot::Renounced => self.fail(&AuthorityError::Renounced),
        }
        *slot = AuthoritySlot::held(Holder {
            account: caller.account,
            via: caller.via,
        });
        self.expected = Some(slot.clone());
    }

    /// The sender, who must be the holder, offers the role to `next`. The all-zero id cancels.
    pub fn nominate(&mut self, slot: &mut AuthoritySlot, next: AccountId) {
        self.require(slot);
        if let AuthoritySlot::Held { nominee, .. } = slot {
            *nominee = (next != AccountId::default()).then_some(next);
        }
        self.expected = Some(slot.clone());
    }

    /// The sender, who must be the pending nominee, takes the role. A key at top level needs
    /// no seed. A PDA reached through its program must carry the seed that derives it there.
    pub fn accept(&mut self, slot: &mut AuthoritySlot, pda_seed: Option<PdaSeed>) {
        let caller = *self.vouched();
        let pending = match slot {
            AuthoritySlot::Unset => self.fail(&AuthorityError::NotInstalled),
            AuthoritySlot::Renounced => self.fail(&AuthorityError::Renounced),
            AuthoritySlot::Held { nominee, .. } => *nominee,
        };
        let Some(nominee) = pending else {
            self.fail(&AuthorityError::NoNomination)
        };
        if nominee != caller.account {
            self.fail(&AuthorityError::NotNominee);
        }
        match (caller.via, pda_seed) {
            (None, None) => {}
            (Some(program), Some(seed))
                if AccountId::for_public_pda(&program, &seed) == caller.account => {}
            _ => self.fail(&AuthorityError::SeedProofFailed),
        }
        *slot = AuthoritySlot::held(Holder {
            account: caller.account,
            via: caller.via,
        });
        self.expected = Some(slot.clone());
    }

    /// Empties the slot for good, nominee included. The sender must be the holder.
    pub fn renounce(&mut self, slot: &mut AuthoritySlot) {
        self.require(slot);
        *slot = AuthoritySlot::Renounced;
        self.expected = Some(slot.clone());
    }

    fn vouched(&self) -> &Caller {
        match &self.caller {
            Some(caller) => caller,
            None => self.fail(&AuthorityError::NotThroughSeat),
        }
    }

    fn fail(&self, error: &AuthorityError) -> ! {
        panic!("{}", error.describe(self.role))
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

/// Runs `handler` behind the hop, the management operations and the slot check.
///
/// At any actor other than the config, the message is forwarded to the config if the
/// sender authorized the transaction. At the config, a forwarded message is unwrapped
/// into a [`Caller`] and the config is loaded. An empty account accepts only the install
/// operation, which creates the config. A management operation then runs on the slot
/// without the handler. Any other message goes to the handler, and the slot is checked
/// against what the context allowed. The config is written back either way.
///
/// # Panics
/// On every refusal, with [`AuthorityError::describe`] as the text.
pub fn guard<R, C, M>(
    input: &ReceiveInput,
    seed: &PdaSeed,
    message: M,
    handler: impl FnOnce(&mut Ctx, &mut C, M),
) -> Response
where
    R: Role,
    C: BorshSerialize + BorshDeserialize + Default + HasSlot<R>,
    M: Hosts,
{
    match try_guard::<R, C, M>(input, seed, message, handler) {
        Ok(response) => response,
        Err(error) => panic!("{}", error.describe(R::NAME)),
    }
}

/// [`guard`] with the guard's own refusals as a value. The context still panics.
///
/// # Errors
/// See [`AuthorityError`].
pub fn try_guard<R, C, M>(
    input: &ReceiveInput,
    seed: &PdaSeed,
    message: M,
    handler: impl FnOnce(&mut Ctx, &mut C, M),
) -> Result<Response, AuthorityError>
where
    R: Role,
    C: BorshSerialize + BorshDeserialize + Default + HasSlot<R>,
    M: Hosts,
{
    let program = input.receiver.program_account_id;
    let config_actor = Actor::new(config_account(program, seed), program);
    if input.receiver != config_actor {
        return seat(input, config_actor, message);
    }
    let (caller, message) = match message.into_forwarded() {
        Ok(hop) => {
            if !input.from_own_program() {
                return Err(AuthorityError::NotOwnProgram);
            }
            let from = input.from.ok_or(AuthorityError::NotOwnProgram)?;
            let caller = Caller {
                account: from.account_id,
                via: hop.via,
            };
            let inner: M =
                borsh::from_slice(&hop.inner).map_err(|_| AuthorityError::MessageDoesNotDecode)?;
            let inner = match inner.into_forwarded() {
                Ok(_) => return Err(AuthorityError::MessageDoesNotDecode),
                Err(message) => message,
            };
            (Some(caller), inner)
        }
        Err(message) => (None, message),
    };
    let op = message.into_op();
    let pre: &[u8] = &input.pre_state;
    let mut config: C = if pre.is_empty() {
        if !matches!(op, Ok(Op::Init)) {
            return Err(AuthorityError::NotInitialized);
        }
        C::default()
    } else {
        borsh::from_slice(pre).map_err(|_| AuthorityError::ConfigDoesNotDecode)?
    };
    let before = HasSlot::<R>::slot(&config).clone();
    let mut ctx = Ctx {
        caller,
        role: R::NAME,
        expected: None,
    };
    match op {
        Ok(Op::Init) => ctx.install(HasSlot::<R>::slot_mut(&mut config)),
        Ok(Op::Nominate { next }) => ctx.nominate(HasSlot::<R>::slot_mut(&mut config), next),
        Ok(Op::Accept { pda_seed }) => ctx.accept(HasSlot::<R>::slot_mut(&mut config), pda_seed),
        Ok(Op::Renounce) => ctx.renounce(HasSlot::<R>::slot_mut(&mut config)),
        Err(message) => handler(&mut ctx, &mut config, message),
    }
    let expected = ctx.expected.unwrap_or(before);
    if *HasSlot::<R>::slot(&config) != expected {
        return Err(AuthorityError::SlotChanged);
    }
    let bytes = borsh::to_vec(&config).map_err(|_| AuthorityError::ConfigDoesNotDecode)?;
    Ok(Response::set_state(bytes))
}

/// Hop 1, at the sender's own actor: vouch and forward.
fn seat<M: Hosts>(
    input: &ReceiveInput,
    config_actor: Actor,
    message: M,
) -> Result<Response, AuthorityError> {
    let message = match message.into_forwarded() {
        Ok(_) => return Err(AuthorityError::ForwardedNotAtConfig),
        Err(message) => message,
    };
    if !input.is_authorized {
        return Err(AuthorityError::MissingSignature);
    }
    let hop = M::forwarded(Forwarded {
        via: input.from.map(|from| from.program_account_id),
        inner: borsh::to_vec(&message).map_err(|_| AuthorityError::MessageDoesNotDecode)?,
    });
    Ok(Response::keep_state().send_sync(config_actor, &hop))
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: AccountId = AccountId::new([1; 32]);
    const OTHER: AccountId = AccountId::new([2; 32]);
    const GOVERNANCE: AccountId = AccountId::new([6; 32]);

    fn caller(account: AccountId, via: Option<AccountId>) -> Caller {
        Caller { account, via }
    }

    fn ctx(caller: Option<Caller>) -> Ctx {
        Ctx {
            caller,
            role: "holder",
            expected: None,
        }
    }

    #[test]
    fn a_key_holder_acts_at_top_level_only() {
        let slot = AuthoritySlot::held(Holder { account: KEY, via: None });
        assert_eq!(slot.check(&caller(KEY, None)), Ok(()));
        assert_eq!(slot.check(&caller(KEY, Some(GOVERNANCE))), Err(AuthorityError::ViaNotAllowed));
        assert_eq!(slot.check(&caller(OTHER, None)), Err(AuthorityError::NotHolder));
    }

    #[test]
    fn a_pda_holder_acts_through_its_program_only() {
        let slot = AuthoritySlot::held(Holder { account: KEY, via: Some(GOVERNANCE) });
        assert_eq!(slot.check(&caller(KEY, Some(GOVERNANCE))), Ok(()));
        assert_eq!(slot.check(&caller(KEY, None)), Err(AuthorityError::ViaNotAllowed));
        assert_eq!(slot.check(&caller(KEY, Some(OTHER))), Err(AuthorityError::ViaNotAllowed));
    }

    #[test]
    fn unset_and_renounced_slots_refuse_everyone() {
        assert_eq!(AuthoritySlot::Unset.check(&caller(KEY, None)), Err(AuthorityError::NotInstalled));
        assert_eq!(AuthoritySlot::Renounced.check(&caller(KEY, None)), Err(AuthorityError::Renounced));
    }

    #[test]
    fn install_records_the_sender_and_what_the_slot_must_look_like() {
        let mut slot = AuthoritySlot::Unset;
        let mut ctx = ctx(Some(caller(KEY, Some(GOVERNANCE))));
        ctx.install(&mut slot);
        let held = AuthoritySlot::held(Holder { account: KEY, via: Some(GOVERNANCE) });
        assert_eq!(slot, held);
        assert_eq!(ctx.expected, Some(held));
    }

    fn held_by_key_with(nominee: Option<AccountId>) -> AuthoritySlot {
        AuthoritySlot::Held {
            holder: Holder { account: KEY, via: None },
            nominee,
        }
    }

    #[test]
    fn nominate_records_replaces_and_cancels() {
        let mut slot = held_by_key_with(None);
        let mut ctx = ctx(Some(caller(KEY, None)));
        ctx.nominate(&mut slot, OTHER);
        assert_eq!(slot.nominee(), Some(OTHER));
        ctx.nominate(&mut slot, GOVERNANCE);
        assert_eq!(slot.nominee(), Some(GOVERNANCE));
        ctx.nominate(&mut slot, AccountId::default());
        assert_eq!(slot.nominee(), None);
        assert_eq!(slot.holder(), Some(Holder { account: KEY, via: None }));
    }

    #[test]
    #[should_panic(expected = "the signing account is not the holder")]
    fn only_the_holder_nominates() {
        let mut slot = held_by_key_with(None);
        ctx(Some(caller(OTHER, None))).nominate(&mut slot, OTHER);
    }

    #[test]
    fn a_key_nominee_accepts_at_top_level() {
        let mut slot = held_by_key_with(Some(OTHER));
        ctx(Some(caller(OTHER, None))).accept(&mut slot, None);
        assert_eq!(slot, AuthoritySlot::held(Holder { account: OTHER, via: None }));
    }

    #[test]
    fn a_pda_nominee_accepts_through_its_program_with_the_seed() {
        let seed = PdaSeed::new([9; 32]);
        let pda = AccountId::for_public_pda(&GOVERNANCE, &seed);
        let mut slot = held_by_key_with(Some(pda));
        ctx(Some(caller(pda, Some(GOVERNANCE)))).accept(&mut slot, Some(seed));
        assert_eq!(slot, AuthoritySlot::held(Holder { account: pda, via: Some(GOVERNANCE) }));
    }

    #[test]
    #[should_panic(expected = "must prove itself with the seed")]
    fn a_pda_nominee_with_the_wrong_seed_is_refused() {
        let pda = AccountId::for_public_pda(&GOVERNANCE, &PdaSeed::new([9; 32]));
        let mut slot = held_by_key_with(Some(pda));
        ctx(Some(caller(pda, Some(GOVERNANCE)))).accept(&mut slot, Some(PdaSeed::new([8; 32])));
    }

    #[test]
    #[should_panic(expected = "must prove itself with the seed")]
    fn a_nominee_reached_through_a_program_without_a_seed_is_refused() {
        let mut slot = held_by_key_with(Some(OTHER));
        ctx(Some(caller(OTHER, Some(GOVERNANCE)))).accept(&mut slot, None);
    }

    #[test]
    #[should_panic(expected = "the signing account is not the nominated holder")]
    fn only_the_nominee_accepts() {
        let mut slot = held_by_key_with(Some(OTHER));
        ctx(Some(caller(GOVERNANCE, None))).accept(&mut slot, None);
    }

    #[test]
    #[should_panic(expected = "no holder nominee is pending")]
    fn accept_without_a_nomination_is_refused() {
        let mut slot = held_by_key_with(None);
        ctx(Some(caller(OTHER, None))).accept(&mut slot, None);
    }

    #[test]
    #[should_panic(expected = "the holder is renounced")]
    fn accept_after_renounce_is_refused() {
        let mut slot = held_by_key_with(Some(OTHER));
        ctx(Some(caller(KEY, None))).renounce(&mut slot);
        assert_eq!(slot, AuthoritySlot::Renounced);
        ctx(Some(caller(OTHER, None))).accept(&mut slot, None);
    }

    #[test]
    #[should_panic(expected = "the holder is already installed")]
    fn install_refuses_a_held_slot() {
        let mut slot = AuthoritySlot::held(Holder { account: KEY, via: None });
        ctx(Some(caller(OTHER, None))).install(&mut slot);
    }

    #[test]
    #[should_panic(expected = "must go through the sender's own actor")]
    fn require_refuses_a_message_that_skipped_the_seat() {
        let slot = AuthoritySlot::held(Holder { account: KEY, via: None });
        ctx(None).require(&slot);
    }
}
