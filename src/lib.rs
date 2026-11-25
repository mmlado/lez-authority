//! Authority primitives for LEZ actor programs.
//!
//! A program keeps its config on one well-known account, the config actor, a PDA of the
//! program id and a seed. Inside that config sits a slot per role, with the role's holder in
//! an [`AuthoritySlot`]. A message that needs a holder goes to the sender's own actor under
//! the program first, the seat.
//! The seat checks `is_authorized` and forwards the message to the config, naming the program
//! that reached it. The config trusts the forward because only this program's own actors
//! reach it with `from_own_program()`, and every one of them runs the same code: [`guard()`].
//! The guard refuses a handler whose response sends the config a library message, and a role
//! whose rules send any actor of the program a message other than its own, so only a seat
//! builds the hop.
//!
//! The program stays a plain LEZ program: its own message enum, its own state, and a handler
//! `fn(&ReceiveInput, Message) -> Response` under `lee_core::actor_entrypoint!`. The
//! [`guard`](macro@guard) attribute puts the guard in front of that handler. The library's
//! own messages, the roles' management operations, the hop, and a role's own messages to
//! another actor, travel as a [`Wire`] message behind a reserved first byte, so the program's
//! enum hosts none of them. A program marks
//! the messages that need a holder, the ones only the config may send, and the ones exempt
//! from a role's consent, through [`Gated`].
//!
//! A program names its roles as a tuple, [`Roles`]: one role, `(A,)`, or two, `(A, B)`.
//! Each keeps its own slot in the config. The first creates the config.
//!
//! This crate is policy-free. Which role a slot plays comes from the consumer through
//! [`Role`], who may install, nominate and renounce through [`Policy`], and what a role's own
//! operations do and which program messages may run, over which state and with which writes,
//! through [`Hooks`].

#![warn(missing_docs)]

// The macros name this crate by its path, which inside it needs this alias.
extern crate self as lez_authority;

/// Derive `HasSlot` from fields marked `#[slot(Role)]`, and `Gated` from messages marked
/// `#[only(Role)]`, `#[from_config]` or `#[exempt(Role)]`. Put [`guard`](macro@guard) on the
/// handler. See [`lez_authority_macros`].
pub use lez_authority_macros::{Gated, HasSlot, guard};

mod role;
mod roles;
mod routing;
mod slot;
mod wire;

pub use lez_authority_core::{
    AuthorityError, AuthoritySlot, HasSlot, Holder, InitGate, Refusal, Role, Slot,
};
pub use role::{Ctx, Hooks, Policy};
pub use roles::Roles;
pub use routing::{config_account, guard, own_actor, seed, try_guard};
pub use slot::Caller;
pub use wire::{Clearance, Forwarded, Gated, LIBRARY_TAG, LibraryMessage, Op, Wire};

#[cfg(doctest)]
mod compile_fail;

#[cfg(test)]
mod tests;
