//! A role's slot: its holder, a standing offer, and the guard's view of a sender.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::AccountId;

use crate::*;

/// Who holds a role, and through which program they act. `via` is `None` for a key that
/// signs at top level, and the owning program for a PDA that is granted by it.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Holder {
    /// The holder's account.
    pub account: AccountId,
    /// The program the holder acts through, or `None` for top level.
    pub via: Option<AccountId>,
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

    /// Whether `caller` is the holder, reached the way the holder acts.
    ///
    /// # Errors
    /// `NotInstalled`, `Renounced`, `NotHolder` for another account, `ViaNotAllowed` for
    /// the holder's account reached through the wrong program.
    pub fn check(&self, caller: &Caller) -> Result<(), AuthorityError> {
        let holder = match self {
            Self::Unset | Self::Offered { .. } => return Err(AuthorityError::NotInstalled),
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
