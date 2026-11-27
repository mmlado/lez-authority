//! What a role is, where its slot lives, and who may install its first holder.

use lee_core::account::AccountId;

use crate::Slot;

/// A role a slot plays, such as admin. Names the slot in panic text.
pub trait Role: 'static {
    /// The role's name, as it appears in a refusal's prefix and in a client's operation
    /// message. The guard knows a role by it, so a program's roles need a name each, and a
    /// guard over two roles of one name does not compile.
    const NAME: &'static str;
    /// The role's name in a refusal's sentence, such as "freeze authority" for the role named
    /// "freeze". By default its name.
    const PROSE_NAME: &'static str = Self::NAME;
    /// The type of the role's slot in the config, [`AuthoritySlot`](crate::AuthoritySlot) for a
    /// role that keeps only its holder.
    type Slot: Slot;
}

/// Where role `R`'s slot lives inside a config. The guard runs the management operations
/// on it and refuses a handler that changed it.
pub trait HasSlot<R: Role> {
    /// The slot.
    fn slot(&self) -> &R::Slot;
    /// The slot, to change it.
    fn slot_mut(&mut self) -> &mut R::Slot;
}

/// Who may install the first holder.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InitGate {
    /// The program's own account. On LEZ 0.4 that is the loader header account, whose key
    /// the deployer holds, so nobody else can win the race after deployment.
    Deployer,
    /// A key chosen at build time, for a program whose header account is not a key.
    Genesis(AccountId),
}
