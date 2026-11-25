//! The policy-free base of lez-authority: what a role is and where its slot lives, the slot's
//! holder and the operations that move it, and the table of refusal codes.
//!
//! A program takes these through `lez-authority`, which re-exports each under the same name.
//! Who may run an operation on a slot is the role's policy, which lives above this crate.

#![warn(missing_docs)]

mod error;
mod role;
mod slot;

pub use error::{AuthorityError, Refusal};
pub use role::{HasSlot, InitGate, Role};
pub use slot::{AuthoritySlot, Holder, Slot};

#[cfg(test)]
mod tests;
