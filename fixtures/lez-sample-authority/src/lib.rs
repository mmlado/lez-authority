//! A role crate for the engine's own tests: the role `Sample`, with the engine re-exported at the
//! root, as every role crate re-exports it.

pub use lez_authority;
pub use lez_authority::{AuthoritySlot, Gated, HasSlot, Policy, Role, Wire, guard};

/// A role that answers to nobody.
pub struct Sample;

impl Role for Sample {
    const NAME: &'static str = "sample";
    type Slot = AuthoritySlot;
}

impl<C: HasSlot<Sample>> Policy<C> for Sample {}
