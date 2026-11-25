//! The guard's view of a sender.

use lee_core::account::AccountId;

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
