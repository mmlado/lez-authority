//! Freeze authority for LEZ actor programs: the freeze role of lez-authority, answering to the
//! admin of lez-admin-authority. A program depends on this crate alone, which re-exports the
//! admin and the engine. It puts the engine's guard on its handler with `roles(Admin, Freeze)`,
//! and its config holds the freeze slot in a field marked `#[slot(Freeze)]`. The admin's
//! `#[admin]` names the admin alone, so a freeze slot under it is never reached.

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
}

impl FreezeError {
    /// Every refusal, for the table test.
    pub const ALL: [Self; 1] = [Self::NotAdminOrFreezeAuthority];

    /// The stable code. Append-only: a number is never reused or renumbered.
    #[must_use]
    pub const fn code(self) -> u16 {
        match self {
            Self::NotAdminOrFreezeAuthority => 208,
        }
    }

    /// The sentence.
    #[must_use]
    pub const fn sentence(self) -> &'static str {
        match self {
            Self::NotAdminOrFreezeAuthority => {
                "the signing account is neither the freeze authority nor the admin"
            }
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
/// standing at that moment can still be accepted, and nobody can withdraw it.
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

/// A client's message for one of the freeze role's management operations, nominate, accept or
/// renounce, to send to the sender's own actor. The admin appoints, replaces and revokes the
/// freeze authority with these, signing at its own seat, while `admin_op` acts on the admin's
/// own slot. The freeze role's `Init` is refused (E137), and before the admin's `Init` every
/// operation is (E101).
#[must_use]
pub fn freeze_authority_op<M>(op: Op) -> Wire<M> {
    Wire::op::<Freeze>(op)
}

#[cfg(test)]
mod tests;
