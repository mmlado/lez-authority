//! Freeze authority for LEZ actor programs: the freeze role of lez-authority, answering to the
//! admin of lez-admin-authority. A program depends on this crate alone, which re-exports the
//! admin and the engine. It puts the engine's guard on its handler with `roles(Admin, Freeze)`,
//! and its config holds the freeze slot in a field marked `#[slot(Freeze)]`. A message that runs
//! through a freeze is marked `#[exempt(Freeze)]`. The admin's `#[admin]` names the admin alone,
//! so a freeze slot under it is never reached and an attribute naming `Freeze` does not compile.
//! A frozen account is marked at an actor of the program of its own, its marker,
//! [`marker_actor`].

#![warn(missing_docs)]

pub use lez_admin_authority;
pub use lez_admin_authority::lez_authority;
pub use lez_admin_authority::{Admin, AdminOp, admin_op};
pub use lez_authority::{
    AuthorityError, AuthoritySlot, Caller, Gated, HasSlot, Holder, Hooks, InitGate, Op, Policy,
    Refusal, Role, Slot, Wire, config_account, guard, seed,
};

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::{AccountId, Actor};
use lee_core::program::SyncMessage;
use lez_authority::{Ctx, own_actor};

/// The freeze role. Its holder, the freeze authority, is only ever appointed by nomination: the
/// admin nominates the first one, and the freeze authority or the admin a successor.
pub struct Freeze;

impl Role for Freeze {
    const NAME: &'static str = "freeze";
    const PROSE_NAME: &'static str = "freeze authority";
    type Slot = FreezeSlot;
}

/// The freeze role's slot: its holder part and whether the whole program is frozen. A program's
/// config holds it in a field marked `#[slot(Freeze)]`, and the admin's `Init` creates it unset,
/// with the program not frozen. A frozen account is marked at its marker, [`marker_actor`].
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct FreezeSlot {
    authority_slot: AuthoritySlot,
    frozen: bool,
}

impl FreezeSlot {
    /// Whether the program is frozen.
    #[must_use]
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }
}

impl Slot for FreezeSlot {
    fn authority_slot(&self) -> &AuthoritySlot {
        &self.authority_slot
    }

    fn authority_slot_mut(&mut self) -> &mut AuthoritySlot {
        &mut self.authority_slot
    }
}

/// What a marker holds while its account is frozen. No handler writes a state that starts with
/// it (E228), so only a freeze operation marks an account.
pub const MARK: [u8; 32] = *b"lez-freeze-authority/mark/v1\0\0\0\0";

/// The marker of `account` in the program whose config actor is `config`: the freeze role's own
/// actor for the account's id. The account is frozen exactly while its marker holds [`MARK`],
/// and any other state there is stray, so a client compares the whole state with it. A
/// transaction declares the marker when it freezes or releases the account, or runs a freezable
/// message with the account as its caller, since the config asks it then.
#[must_use]
pub fn marker_actor(config: Actor, account: AccountId) -> Actor {
    own_actor::<Freeze>(config, account.value())
}

/// What the config asks of an account's marker, as the freeze role's own message.
#[derive(BorshSerialize, BorshDeserialize)]
enum MarkerMessage {
    /// Writes the mark, over any stray state.
    Mark,
    /// Leaves the marker empty.
    Unmark,
    /// Refuses while the account is frozen.
    Check,
}

/// `message` to `account`'s marker.
fn to_marker(ctx: &Ctx, account: AccountId, message: &MarkerMessage) -> SyncMessage {
    let message = borsh::to_vec(message).expect("a marker message encodes");
    ctx.own_message::<Freeze>(*account.value(), message)
}

/// The freeze role's own refusals, printed as `lez-freeze-authority/freeze E<code>: <sentence>`.
/// The codes keep v0.2's numbers where the meaning carries over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeError {
    /// The sender is neither the freeze authority nor the admin.
    NotAdminOrFreezeAuthority,
    /// A freeze of a program that is frozen already.
    AlreadyFrozen,
    /// A release of a program that is not frozen.
    NotFrozen,
    /// A freeze of an account that is frozen already.
    AccountAlreadyFrozen,
    /// A release of an account that is not frozen.
    AccountNotFrozen,
    /// A program message while the program is frozen.
    Frozen,
    /// A program message whose caller is a frozen account.
    AccountFrozen,
    /// A freeze of the all-zero id.
    ZeroAccount,
    /// A program message at a frozen account's marker.
    AtMarker,
    /// A handler's write that starts with the mark.
    WritesMark,
}

impl FreezeError {
    /// Every refusal, for the table test.
    pub const ALL: [Self; 10] = [
        Self::NotAdminOrFreezeAuthority,
        Self::AlreadyFrozen,
        Self::NotFrozen,
        Self::AccountAlreadyFrozen,
        Self::AccountNotFrozen,
        Self::Frozen,
        Self::AccountFrozen,
        Self::ZeroAccount,
        Self::AtMarker,
        Self::WritesMark,
    ];

    /// The stable code. Append-only: a number is never reused or renumbered.
    #[must_use]
    pub const fn code(self) -> u16 {
        match self {
            Self::NotAdminOrFreezeAuthority => 208,
            Self::AlreadyFrozen => 211,
            Self::NotFrozen => 212,
            Self::AccountAlreadyFrozen => 213,
            Self::AccountNotFrozen => 214,
            Self::Frozen => 217,
            Self::AccountFrozen => 218,
            Self::ZeroAccount => 225,
            Self::AtMarker => 227,
            Self::WritesMark => 228,
        }
    }

    /// The sentence.
    #[must_use]
    pub const fn sentence(self) -> &'static str {
        match self {
            Self::NotAdminOrFreezeAuthority => {
                "the signing account is neither the freeze authority nor the admin"
            }
            Self::AlreadyFrozen => "the program is already frozen",
            Self::NotFrozen => "the program is not frozen",
            Self::AccountAlreadyFrozen => "the account is already frozen",
            Self::AccountNotFrozen => "the account is not frozen",
            Self::Frozen => "the program is frozen",
            Self::AccountFrozen => "the signing account is frozen",
            Self::ZeroAccount => "the all-zero id names no account",
            Self::AtMarker => "the actor is a frozen account's marker",
            Self::WritesMark => "the new state starts with the freeze's mark",
        }
    }
}

impl From<FreezeError> for AuthorityError {
    fn from(error: FreezeError) -> Self {
        Self::Role {
            krate: "lez-freeze-authority",
            code: error.code(),
            sentence: error.sentence(),
        }
    }
}

/// The freeze authority answers to the admin. Either of them nominates and renounces, and a
/// renounce leaves the slot unset for the admin to fill again. Once the admin is renounced,
/// nobody can nominate where the slot has no holder, so an emptied slot stays empty. An offer
/// standing at that moment can still be accepted, and nobody can withdraw it. Only the freeze
/// authority freezes and releases, the program or one account, and a freeze operation that would
/// change nothing is refused.
///
/// While the program is frozen, every program message is refused (E217), but for one marked
/// `#[exempt(Freeze)]`: at the config, and at the program's other actors through their request
/// for clearance, so a refused message there changes nothing. A program message whose caller is
/// a frozen account is refused the same way (E218), after the check for a frozen program, so a
/// frozen program answers E217 first. The config asks the caller's marker, [`marker_actor`],
/// which refuses in the same transaction while it holds the mark. At the config that comes after
/// the handler, so a handler that fails first is refused with its own error. A message without a
/// caller, such as one sent straight to the config or a credit at the actor of a recipient that
/// did not sign the transaction, asks no marker. It is refused while the program is frozen
/// (E217), and otherwise only at a marker or for a write of the mark, as below.
///
/// No program message runs at a marker while it holds the mark (E227), not even an exempt one,
/// and no handler writes a state that starts with the mark at any actor (E228). So only the
/// freeze authority marks an account.
impl<C> Policy<C> for Freeze
where
    C: HasSlot<Freeze> + HasSlot<Admin>,
{
    const BY_NOMINATION_ONLY: bool = true;

    fn may_nominate(ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        freeze_authority_or_admin(ctx, config)
    }

    fn may_renounce(ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        freeze_authority_or_admin(ctx, config)
    }

    fn renounced() -> AuthoritySlot {
        AuthoritySlot::Unset
    }
}

impl<C> Hooks<C> for Freeze
where
    C: HasSlot<Freeze> + HasSlot<Admin>,
{
    const REFUSES_MESSAGES: bool = true;
    const WATCHES_STATE: bool = true;

    fn operate(
        ctx: &Ctx,
        config: &C,
        op: &[u8],
    ) -> Result<(FreezeSlot, Vec<SyncMessage>), Refusal> {
        let op: FreezeOp = borsh::from_slice(op)
            .map_err(|_| Refusal::of::<Freeze>(AuthorityError::MessageDoesNotDecode))?;
        let slot = HasSlot::<Freeze>::slot(config);
        ctx.check::<Freeze>(slot.authority_slot())?;
        let refuse = |error: FreezeError| Err(Refusal::of::<Freeze>(error.into()));
        let mut slot = slot.clone();
        let sends = match op {
            FreezeOp::Freeze if slot.frozen => return refuse(FreezeError::AlreadyFrozen),
            FreezeOp::Release if !slot.frozen => return refuse(FreezeError::NotFrozen),
            FreezeOp::Freeze => {
                slot.frozen = true;
                Vec::new()
            }
            FreezeOp::Release => {
                slot.frozen = false;
                Vec::new()
            }
            FreezeOp::FreezeAccount { account } if account == AccountId::default() => {
                return refuse(FreezeError::ZeroAccount);
            }
            FreezeOp::FreezeAccount { account } => {
                vec![to_marker(ctx, account, &MarkerMessage::Mark)]
            }
            FreezeOp::ReleaseAccount { account } => {
                vec![to_marker(ctx, account, &MarkerMessage::Unmark)]
            }
        };
        Ok((slot, sends))
    }

    fn may_run(ctx: &Ctx, config: &C) -> Result<Vec<SyncMessage>, Refusal> {
        if HasSlot::<Freeze>::slot(config).frozen {
            return Err(Refusal::of::<Freeze>(FreezeError::Frozen.into()));
        }
        // An account is frozen whichever program reached it, so the via is not compared.
        let check = ctx
            .caller()
            .map(|caller| to_marker(ctx, caller.account(), &MarkerMessage::Check));
        Ok(check.into_iter().collect())
    }

    fn receive(_ctx: &Ctx, state: &[u8], message: &[u8]) -> Result<Option<Vec<u8>>, Refusal> {
        let message: MarkerMessage = borsh::from_slice(message)
            .map_err(|_| Refusal::of::<Freeze>(AuthorityError::MessageDoesNotDecode))?;
        let refuse = |error: FreezeError| Err(Refusal::of::<Freeze>(error.into()));
        let marked = state == MARK;
        match message {
            MarkerMessage::Mark if marked => refuse(FreezeError::AccountAlreadyFrozen),
            MarkerMessage::Mark => Ok(Some(MARK.to_vec())),
            MarkerMessage::Unmark if marked => Ok(Some(Vec::new())),
            MarkerMessage::Unmark => refuse(FreezeError::AccountNotFrozen),
            MarkerMessage::Check if marked => refuse(FreezeError::AccountFrozen),
            MarkerMessage::Check => Ok(None),
        }
    }

    fn may_handle(_ctx: &Ctx, state: &[u8]) -> Result<(), Refusal> {
        if state == MARK {
            return Err(Refusal::of::<Freeze>(FreezeError::AtMarker.into()));
        }
        Ok(())
    }

    fn may_write(_ctx: &Ctx, _before: &[u8], after: Option<&[u8]>) -> Result<(), Refusal> {
        if after.is_some_and(|after| after.starts_with(&MARK)) {
            return Err(Refusal::of::<Freeze>(FreezeError::WritesMark.into()));
        }
        Ok(())
    }
}

/// Passes the freeze authority and the admin. A holder's account that came the wrong way is
/// told so, anyone else gets E208.
fn freeze_authority_or_admin<C>(ctx: &Ctx, config: &C) -> Result<(), Refusal>
where
    C: HasSlot<Freeze> + HasSlot<Admin>,
{
    let as_freeze = ctx.check::<Freeze>(HasSlot::<Freeze>::slot(config).authority_slot());
    let as_admin = ctx.check::<Admin>(HasSlot::<Admin>::slot(config));
    let (Err(as_freeze), Err(as_admin)) = (as_freeze, as_admin) else {
        return Ok(());
    };
    for refusal in [as_freeze, as_admin] {
        if matches!(
            refusal.error,
            AuthorityError::NotThroughSeat | AuthorityError::ViaNotAllowed
        ) {
            return Err(refusal);
        }
    }
    Err(Refusal::of::<Freeze>(
        FreezeError::NotAdminOrFreezeAuthority.into(),
    ))
}

/// A client's message for nominate, accept or renounce on the freeze role's slot, to send to the
/// sender's own actor. The admin appoints, replaces and revokes the freeze authority with these,
/// signing at its own seat, while `admin_op` acts on the admin's own slot. The freeze role's
/// `Init` is refused (E137), and before the admin's `Init` every operation is (E101). Freeze and
/// release go through [`freeze_op`].
#[must_use]
pub fn freeze_authority_op<M>(op: Op) -> Wire<M> {
    Wire::op::<Freeze>(op)
}

/// A freeze operation. Only the freeze authority sends one, through its own seat, and the guard
/// runs it at the config.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum FreezeOp {
    /// Freezes the program: every program message is then refused (E217), at the config and at
    /// the program's other actors, but for the exempt ones. Refused with E211 when it is frozen
    /// already.
    Freeze,
    /// Releases the program. Refused with E212 when it is not frozen.
    Release,
    /// Freezes one account, and runs whether the program is frozen or not. The config writes the
    /// mark at the account's marker, [`marker_actor`], over whatever state sits there. Every
    /// program message with that account as its caller is then refused (E218), at the config and
    /// at the program's other actors, but for the exempt ones. No program message runs at the
    /// marker, exempt or not (E227). While the program is frozen, a message with that account as
    /// its caller is refused with E217 instead. Its management operations still run. Refused with
    /// E213 when the account is frozen already, and with E225 for the all-zero id.
    FreezeAccount {
        /// The account to freeze.
        account: AccountId,
    },
    /// Releases one account and leaves its marker empty. Refused with E214 when it is not
    /// frozen.
    ReleaseAccount {
        /// The account to release.
        account: AccountId,
    },
}

/// A client's message for a freeze operation, to send to the sender's own actor. Before the
/// admin's `Init` it is refused (E101).
#[must_use]
pub fn freeze_op<M>(op: FreezeOp) -> Wire<M> {
    let op = borsh::to_vec(&op).expect("a freeze operation encodes");
    Wire::op::<Freeze>(Op::Own(op))
}

#[cfg(test)]
mod tests;
