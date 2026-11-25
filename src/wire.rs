//! The messages a guarded handler receives: the program's own and the library's.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::account::AccountId;
use lee_core::program::PdaSeed;
use std::io::{Read, Write};

use crate::*;

/// A management operation on a role's slot. A client sends it as a library message, the
/// guard runs it under the role's [`Policy`], and the handler never sees it. Its encoding is
/// Borsh's, except that the bytes of [`Op::Own`] carry no length and run to the end of the
/// message, so an `Op` is always the last thing in one.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub enum Op {
    /// Makes the sender the holder of a slot that has none. A standing offer stays.
    Init,
    /// Offers the role to `next`, by default from the holder. The all-zero id cancels, a later
    /// offer replaces an earlier one. Nothing moves until `next` accepts.
    Nominate {
        /// The account offered the role.
        next: AccountId,
    },
    /// The pending nominee takes the role, through its own seat. A key accepts at top level.
    /// A PDA accepts through the program that derives it, and carries that seed as proof.
    Accept {
        /// The seed that derives the nominee under the program that reached its seat.
        pda_seed: Option<PdaSeed>,
    },
    /// Gives the role up, nominee included. By default the sender must be the holder, and the
    /// holder part stays renounced for good. Whatever else the role keeps in its slot stays.
    Renounce,
    /// One of the operations the role defines for itself, such as a freeze, encoded as the
    /// role encodes it. [`Hooks::operate`] runs it.
    Own(
        // A length in front would hide the role's type from an IDL, so the bytes run to the end.
        #[borsh(
            serialize_with = "rest::serialize",
            deserialize_with = "rest::deserialize"
        )]
        Vec<u8>,
    ),
}

/// The bytes of [`Op::Own`] and of [`LibraryMessage::Own`], to the end of the message.
mod rest {
    use std::io::{Read, Result, Write};

    pub fn serialize<W: Write>(bytes: &[u8], writer: &mut W) -> Result<()> {
        writer.write_all(bytes)
    }

    pub fn deserialize<R: Read>(reader: &mut R) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes)?;
        Ok(bytes)
    }
}

/// A message the sender's own actor forwarded to the config, naming the program that
/// reached it. Clients never send it: the config accepts it only from an actor of the same
/// program.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub struct Forwarded {
    /// The program that reached the sender's actor, or `None` at top level.
    pub via: Option<AccountId>,
    /// The original message, as it arrived at the seat.
    pub inner: Vec<u8>,
}

/// The library's own messages, which never reach the program's handler.
#[derive(BorshSerialize, BorshDeserialize, Clone, Debug, PartialEq, Eq)]
pub enum LibraryMessage {
    /// A management operation on the slot of the role with this name.
    Op {
        /// The role's [`Role::NAME`].
        role: String,
        /// The operation.
        op: Op,
    },
    /// The hop from a seat to the config.
    Forwarded(Forwarded),
    /// A request for clearance, from the actor of the program that handled a program message to
    /// the config.
    Clearance(Clearance),
    /// A role's own message, from the config to the role's own actor for `key`, which hands it
    /// to the role's [`Hooks::receive`] with that actor's state. The handler never sees it. A
    /// role builds one with [`Ctx::own_message`].
    Own {
        /// The role's [`Role::NAME`].
        role: String,
        /// The key the role's own actor is derived from, with the role and the config, as
        /// [`own_actor`] derives it.
        key: [u8; 32],
        /// The message, encoded as the role encodes it. Its bytes carry no length and run to the
        /// end of the message, as those of [`Op::Own`] do.
        #[borsh(
            serialize_with = "rest::serialize",
            deserialize_with = "rest::deserialize"
        )]
        message: Vec<u8>,
    },
}

/// The role a role's own message names, if `bytes` are one.
pub(crate) fn own_message_role(bytes: &[u8]) -> Option<String> {
    if bytes.first() != Some(&LIBRARY_TAG) {
        return None;
    }
    match borsh::from_slice(bytes) {
        Ok(Wire::<()>::Library(LibraryMessage::Own { role, .. })) => Some(role),
        _ => None,
    }
}

/// What a request for clearance says: only whether the sending actor's own account authorized
/// the transaction. The config takes the account from the sender, since a private actor's call
/// blinds the sender but not the body.
#[derive(BorshSerialize, BorshDeserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Clearance {
    /// Whether the sending actor's account authorized the transaction, as the runtime says at
    /// that actor, whoever sent the message there.
    pub authorized: bool,
}

/// The first byte of a library message. A program's message enum must stay below 255
/// variants, so that none of its messages starts with it.
pub const LIBRARY_TAG: u8 = 255;

/// What arrives at a guarded handler: one of the program's own messages, encoded as the
/// program encodes it, or a library message behind [`LIBRARY_TAG`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Wire<M> {
    /// One of the program's messages.
    Program(M),
    /// One of the library's.
    Library(LibraryMessage),
}

impl<M> Wire<M> {
    /// A client's message for one of role `R`'s management operations.
    #[must_use]
    pub fn op<R: Role>(op: Op) -> Self {
        Self::Library(LibraryMessage::Op {
            role: R::NAME.into(),
            op,
        })
    }
}

impl<M> From<M> for Wire<M> {
    fn from(message: M) -> Self {
        Self::Program(message)
    }
}

impl<M: BorshSerialize> BorshSerialize for Wire<M> {
    fn serialize<W: Write>(&self, writer: &mut W) -> std::io::Result<()> {
        match self {
            Self::Program(message) => message.serialize(writer),
            Self::Library(message) => {
                writer.write_all(&[LIBRARY_TAG])?;
                message.serialize(writer)
            }
        }
    }
}

impl<M: BorshDeserialize> BorshDeserialize for Wire<M> {
    fn deserialize_reader<R: Read>(reader: &mut R) -> std::io::Result<Self> {
        let mut first = [0u8; 1];
        reader.read_exact(&mut first)?;
        if first[0] == LIBRARY_TAG {
            LibraryMessage::deserialize_reader(reader).map(Self::Library)
        } else {
            M::deserialize_reader(&mut (&first[..]).chain(reader)).map(Self::Program)
        }
    }
}

/// What the program's messages declare to the guard. Derive it with `#[only(Role)]` on a
/// message that needs a role's holder, the way `onlyOwner` marks a Solidity function,
/// `#[from_config]` on one that only the config account may send, and `#[exempt(Role)]` on one
/// that runs where that role's [`Hooks::may_run`] would refuse it, though a role that watches
/// state still looks at the state around its handler. The derive compares roles by
/// [`Role::NAME`], as the guard does, so a role crate linked twice is still one role. A
/// hand-written impl does the same and lists its roles in [`Gated::MARKED`].
///
/// A marker names a role, so a marker naming any other type does not compile.
///
/// The role must be one of the guard's roles, so a marker naming any other role does not
/// compile either. The guard attribute refuses it where `cargo check` sees it, and a guard
/// called by hand at `cargo build`.
pub trait Gated {
    /// The names of the roles this enum's markers name, which the guard compares with its own
    /// roles at compile time. The derive fills it. A hand-written impl that leaves it empty
    /// goes unchecked.
    const MARKED: &'static [&'static str] = &[];

    /// The names of the roles this enum's `#[exempt]` markers name, each of which the guard
    /// checks at compile time refuses program messages. The derive fills it. A hand-written impl
    /// that leaves it empty goes unchecked. An exemption from a role that refuses nothing does
    /// not compile.
    const EXEMPTED: &'static [&'static str] = &[];

    /// Whether this message needs role `R`'s holder. The guard then takes it only through
    /// the sender's own actor and checks the holder before the handler runs. A message that
    /// needs two roles needs one caller holding both.
    fn needs<R: Role>(&self) -> bool {
        false
    }

    /// Whether only the config account may send this message, as it does to the program's
    /// other actors.
    fn only_from_config(&self) -> bool {
        false
    }

    /// Whether this message runs where role `R`'s [`Hooks::may_run`] would refuse it, as an
    /// exempt message runs through a freeze. The mark covers this message only.
    fn exempt<R: Role>(&self) -> bool {
        false
    }
}
