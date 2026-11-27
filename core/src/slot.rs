//! A role's slot: its holder, a standing offer, and the operations that move the holder.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::AccountId;
use lee_core::program::PdaSeed;

use crate::AuthorityError;

/// Who holds a role, and through which program they act. `via` is `None` for a key that
/// signs at top level, and the owning program for a PDA that is granted by it.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Holder {
    /// The holder's account.
    pub account: AccountId,
    /// The program the holder acts through, or `None` for top level.
    pub via: Option<AccountId>,
}

/// The holder part of a role's slot, and the whole slot of a role that keeps only its holder:
/// nobody yet, nobody yet with a standing offer, a holder with at most one pending nominee,
/// or renounced for good.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, Default, PartialEq, Eq)]
pub enum AuthoritySlot {
    /// No holder yet. The state of a config nobody initialized.
    #[default]
    Unset,
    /// Held, with the holder's standing offer of the role to `nominee` if any.
    Held {
        /// The holder.
        holder: Holder,
        /// The account the role is offered to, until it accepts or the offer changes.
        nominee: Option<AccountId>,
    },
    /// Renounced. Terminal.
    Renounced,
    /// No holder yet, with a standing offer of the role to `nominee`. A slot gets one where
    /// the role's policy lets someone who is not the holder nominate.
    Offered {
        /// The account offered the role, until it accepts or the offer changes.
        nominee: AccountId,
    },
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

    /// The holder, or `None` when the slot has none.
    #[must_use]
    pub fn holder(&self) -> Option<Holder> {
        match self {
            Self::Held { holder, .. } => Some(*holder),
            Self::Unset | Self::Renounced | Self::Offered { .. } => None,
        }
    }

    /// The pending nominee, if any.
    #[must_use]
    pub fn nominee(&self) -> Option<AccountId> {
        match self {
            Self::Held { nominee, .. } => *nominee,
            Self::Offered { nominee } => Some(*nominee),
            Self::Unset | Self::Renounced => None,
        }
    }

    /// Whether `account` is the holder, reached the way the holder acts: through the program
    /// `via`, or at top level for `None`.
    ///
    /// # Errors
    /// `NotInstalled`, `Renounced`, `NotHolder` for another account, `ViaNotAllowed` for
    /// the holder's account reached through the wrong program.
    // By reference, compared in place: by value the copies cost a guest about 220 cycles a check.
    pub fn check(
        &self,
        account: &AccountId,
        via: &Option<AccountId>,
    ) -> Result<(), AuthorityError> {
        let holder = match self {
            Self::Unset | Self::Offered { .. } => return Err(AuthorityError::NotInstalled),
            Self::Renounced => return Err(AuthorityError::Renounced),
            Self::Held { holder, .. } => holder,
        };
        if holder.account != *account {
            return Err(AuthorityError::NotHolder);
        }
        if holder.via != *via {
            return Err(AuthorityError::ViaNotAllowed);
        }
        Ok(())
    }

    /// Install: `holder` takes a slot that has none, and a standing offer stays pending. It
    /// checks nobody, since who may install is the role's policy.
    ///
    /// # Errors
    /// `AlreadyInstalled` for a held slot, `Renounced` for a renounced one. A refused install
    /// leaves the slot as it was.
    pub fn install(&mut self, holder: Holder) -> Result<(), AuthorityError> {
        let nominee = match self {
            Self::Unset => None,
            Self::Offered { nominee } => Some(*nominee),
            Self::Held { .. } => return Err(AuthorityError::AlreadyInstalled),
            Self::Renounced => return Err(AuthorityError::Renounced),
        };
        *self = Self::Held { holder, nominee };
        Ok(())
    }

    /// Nominate: offers the role to `next`, or withdraws the offer for `None`. On a slot with
    /// no holder the offer is a standing one. It checks nobody, since who may nominate is the
    /// role's policy.
    ///
    /// # Errors
    /// `Renounced` for a renounced slot, which stays as it was.
    pub fn nominate(&mut self, next: Option<AccountId>) -> Result<(), AuthorityError> {
        match self {
            Self::Held { nominee, .. } => *nominee = next,
            Self::Unset | Self::Offered { .. } => {
                *self = next.map_or(Self::Unset, |nominee| Self::Offered { nominee });
            }
            Self::Renounced => return Err(AuthorityError::Renounced),
        }
        Ok(())
    }

    /// Accept: the pending nominee, `by`, becomes the holder, and the offer ends. A nominee
    /// reached through a program proves itself with `pda_seed`, the seed that derives its
    /// account under that program, and a nominee at top level gives none.
    ///
    /// # Errors
    /// In this order: `Renounced`, `NoNomination`, `NotNominee` for another account, and
    /// `SeedProofFailed` for a seed that is missing, wrong or given at top level. A refused
    /// accept leaves the slot as it was.
    pub fn accept(&mut self, by: Holder, pda_seed: Option<PdaSeed>) -> Result<(), AuthorityError> {
        let pending = match self {
            Self::Unset => None,
            Self::Renounced => return Err(AuthorityError::Renounced),
            Self::Held { nominee, .. } => *nominee,
            Self::Offered { nominee } => Some(*nominee),
        };
        let Some(nominee) = pending else {
            return Err(AuthorityError::NoNomination);
        };
        if nominee != by.account {
            return Err(AuthorityError::NotNominee);
        }
        match (by.via, pda_seed) {
            (None, None) => {}
            (Some(program), Some(seed))
                if AccountId::for_public_pda(&program, &seed) == by.account => {}
            _ => return Err(AuthorityError::SeedProofFailed),
        }
        *self = Self::held(by);
        Ok(())
    }

    /// Renounce: the slot's holder part becomes `left`, which is [`AuthoritySlot::Renounced`]
    /// for a role renounced for good. Who may renounce, and what it leaves, is the role's
    /// policy.
    pub fn renounce(&mut self, left: Self) {
        *self = left;
    }
}

/// What a role keeps in its slot: the holder part, which the guard's operations and checks
/// work on, and whatever else the role keeps there, such as the accounts a freeze authority
/// froze. A role that keeps only its holder takes [`AuthoritySlot`] itself. The guard
/// refuses a handler that changed any part of a slot.
pub trait Slot: PartialEq {
    /// The holder part.
    fn authority_slot(&self) -> &AuthoritySlot;
    /// The holder part, to change it.
    fn authority_slot_mut(&mut self) -> &mut AuthoritySlot;
}

impl Slot for AuthoritySlot {
    fn authority_slot(&self) -> &AuthoritySlot {
        self
    }

    fn authority_slot_mut(&mut self) -> &mut AuthoritySlot {
        self
    }
}
