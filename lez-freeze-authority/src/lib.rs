//! Freeze authority for LEZ actor programs: the freeze role of lez-authority, answering to the
//! admin of lez-admin-authority. A program depends on this crate alone, which re-exports the
//! admin and the engine. It puts the engine's guard on its handler with `roles(Admin, Freeze)`,
//! and its config holds the freeze slot in a field marked `#[slot(Freeze)]`. A message that runs
//! through a freeze is marked `#[exempt(Freeze)]`. The admin's `#[admin]` names the admin alone,
//! so a freeze slot under it is never reached and a marker naming `Freeze` does not compile.

#![warn(missing_docs)]

pub use lez_admin_authority;
pub use lez_admin_authority::lez_authority;
pub use lez_admin_authority::{Admin, AdminOp, admin_op};
pub use lez_authority::{
    AuthorityError, AuthoritySlot, Caller, Gated, HasSlot, Holder, InitGate, Op, Policy, Refusal,
    Role, Slot, Wire, config_account, guard, seed,
};

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::AccountId;
use lez_authority::Ctx;

/// The freeze role. Its holder, the freeze authority, is only ever appointed by nomination: the
/// admin nominates the first one, and the freeze authority or the admin a successor.
pub struct Freeze;

impl Role for Freeze {
    const NAME: &'static str = "freeze";
    const PROSE_NAME: &'static str = "freeze authority";
    type Slot = FreezeSlot;
}

/// The freeze role's slot: its holder part, whether the whole program is frozen, and the
/// accounts frozen one by one, sorted. A program's config holds it in a field marked
/// `#[slot(Freeze)]`, and the admin's `Init` creates it unset, with nothing frozen.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct FreezeSlot {
    authority_slot: AuthoritySlot,
    frozen: bool,
    frozen_accounts: Vec<AccountId>,
}

impl FreezeSlot {
    /// Whether the program is frozen.
    #[must_use]
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// The accounts frozen one by one, sorted.
    #[must_use]
    pub fn frozen_accounts(&self) -> &[AccountId] {
        &self.frozen_accounts
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
}

impl FreezeError {
    /// Every refusal, for the table test.
    pub const ALL: [Self; 8] = [
        Self::NotAdminOrFreezeAuthority,
        Self::AlreadyFrozen,
        Self::NotFrozen,
        Self::AccountAlreadyFrozen,
        Self::AccountNotFrozen,
        Self::Frozen,
        Self::AccountFrozen,
        Self::ZeroAccount,
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
/// While the program is frozen, every program message is refused (E217), but for one marked
/// `#[exempt(Freeze)]`: at the config, and at the program's other actors through their request
/// for clearance, so a refused message there changes nothing.
impl<C> Policy<C> for Freeze
where
    C: HasSlot<Freeze> + HasSlot<Admin>,
{
    const BY_NOMINATION_ONLY: bool = true;
    const REFUSES_MESSAGES: bool = true;

    fn may_nominate(ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        freeze_authority_or_admin(ctx, config)
    }

    fn may_renounce(ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        freeze_authority_or_admin(ctx, config)
    }

    fn renounced() -> AuthoritySlot {
        AuthoritySlot::Unset
    }

    fn operate(ctx: &Ctx, config: &C, op: &[u8]) -> Result<FreezeSlot, Refusal> {
        let op: FreezeOp = borsh::from_slice(op)
            .map_err(|_| Refusal::of::<Freeze>(AuthorityError::MessageDoesNotDecode))?;
        let slot = HasSlot::<Freeze>::slot(config);
        ctx.check::<Freeze>(slot.authority_slot())?;
        let refuse = |error: FreezeError| Err(Refusal::of::<Freeze>(error.into()));
        let mut slot = slot.clone();
        match op {
            FreezeOp::Freeze if slot.frozen => return refuse(FreezeError::AlreadyFrozen),
            FreezeOp::Release if !slot.frozen => return refuse(FreezeError::NotFrozen),
            FreezeOp::Freeze => slot.frozen = true,
            FreezeOp::Release => slot.frozen = false,
            FreezeOp::FreezeAccount { account } if account == AccountId::default() => {
                return refuse(FreezeError::ZeroAccount);
            }
            FreezeOp::FreezeAccount { account } => {
                match slot.frozen_accounts.binary_search(&account) {
                    Ok(_) => return refuse(FreezeError::AccountAlreadyFrozen),
                    Err(at) => slot.frozen_accounts.insert(at, account),
                }
            }
            FreezeOp::ReleaseAccount { account } => {
                match slot.frozen_accounts.binary_search(&account) {
                    Ok(at) => {
                        slot.frozen_accounts.remove(at);
                    }
                    Err(_) => return refuse(FreezeError::AccountNotFrozen),
                }
            }
        }
        Ok(slot)
    }

    fn may_run(_ctx: &Ctx, config: &C) -> Result<(), Refusal> {
        if HasSlot::<Freeze>::slot(config).frozen {
            return Err(Refusal::of::<Freeze>(FreezeError::Frozen.into()));
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
    /// Freezes one account, whether the program is frozen or not. Refused with E213 when it is
    /// frozen already, and with E225 for the all-zero id.
    FreezeAccount {
        /// The account to freeze.
        account: AccountId,
    },
    /// Releases one account. Refused with E214 when it is not frozen.
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
