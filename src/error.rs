//! Why a message was refused, and the refusal as a value.

use crate::*;

/// Why a message was refused. It becomes a panic, which is how a handler fails on LEZ, with
/// a stable code in front of the sentence: `lez-authority/<role> E103: the signing account
/// is not the <role>`, the sentence naming the role by its prose name. A role crate's own
/// refusals print with that crate's prefix. A code never changes meaning. New refusals take
/// new codes, retired ones keep theirs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthorityError {
    /// The config account holds no state yet, and the message is not the root role's install.
    NotInitialized,
    /// The config account holds bytes that do not decode as the config.
    ConfigDoesNotDecode,
    /// A message does not encode or decode, names a role the program does not have, or is an
    /// operation its role does not define.
    MessageDoesNotDecode,
    /// The sender did not authorize the transaction at its own actor.
    MissingSignature,
    /// A forwarded message came from outside this program.
    NotOwnProgram,
    /// A forwarded message arrived somewhere other than the config account.
    ForwardedNotAtConfig,
    /// A message that needs a holder was sent straight to the config.
    NotThroughSeat,
    /// The slot holds nobody yet.
    NotInstalled,
    /// The slot already holds someone.
    AlreadyInstalled,
    /// Init came from an account other than the program's own, or not at top level.
    NotDeployer,
    /// Init came from an account other than the genesis key, or not at top level.
    NotGenesis,
    /// No nominee is pending.
    NoNomination,
    /// The caller is not the pending nominee.
    NotNominee,
    /// A nominee reached through a program did not prove itself with the seed that derives it.
    SeedProofFailed,
    /// The slot was renounced, for good.
    Renounced,
    /// The caller is not the holder.
    NotHolder,
    /// The caller is the holder's account but reached its actor through the wrong program.
    ViaNotAllowed,
    /// The program's handler changed a slot.
    SlotChanged,
    /// A message only the config sends came from somewhere else, or reached the config.
    NotFromConfig,
    /// A handler sent a library message to the config account.
    LibrarySend,
    /// A request for clearance came from outside this program.
    ClearanceFromOutside,
    /// Init came for a role whose holder is only ever appointed by nomination.
    ByNominationOnly,
    /// A request for clearance arrived somewhere other than the config account.
    ClearanceNotAtConfig,
    /// A refusal of a role's own, printed with its crate's prefix.
    Role {
        /// The role crate's name, the prefix of the text.
        krate: &'static str,
        /// The code, which the role crate keeps stable.
        code: u16,
        /// The sentence.
        sentence: &'static str,
    },
}

impl AuthorityError {
    /// Every refusal of the engine's own, for the table test.
    pub const ALL: [Self; 23] = [
        Self::NotInitialized,
        Self::ConfigDoesNotDecode,
        Self::MessageDoesNotDecode,
        Self::MissingSignature,
        Self::NotOwnProgram,
        Self::ForwardedNotAtConfig,
        Self::NotThroughSeat,
        Self::NotInstalled,
        Self::AlreadyInstalled,
        Self::NotDeployer,
        Self::NotGenesis,
        Self::NoNomination,
        Self::NotNominee,
        Self::SeedProofFailed,
        Self::Renounced,
        Self::NotHolder,
        Self::ViaNotAllowed,
        Self::SlotChanged,
        Self::NotFromConfig,
        Self::LibrarySend,
        Self::ClearanceFromOutside,
        Self::ByNominationOnly,
        Self::ClearanceNotAtConfig,
    ];

    /// The stable code. Append-only: a number is never reused or renumbered.
    #[must_use]
    pub const fn code(&self) -> u16 {
        match self {
            Self::NotInitialized => 101,
            Self::Renounced => 102,
            Self::NotHolder => 103,
            Self::MissingSignature => 104,
            Self::SeedProofFailed => 107,
            Self::MessageDoesNotDecode => 108,
            Self::ConfigDoesNotDecode => 109,
            Self::NoNomination => 113,
            Self::NotNominee => 114,
            Self::NotGenesis => 115,
            Self::NotDeployer => 117,
            Self::AlreadyInstalled => 118,
            Self::NotOwnProgram => 119,
            Self::ViaNotAllowed => 120,
            Self::ForwardedNotAtConfig => 121,
            Self::NotThroughSeat => 122,
            Self::NotFromConfig => 123,
            Self::SlotChanged => 126,
            Self::NotInstalled => 131,
            Self::LibrarySend => 135,
            Self::ClearanceFromOutside => 136,
            Self::ByNominationOnly => 137,
            Self::ClearanceNotAtConfig => 138,
            Self::Role { code, .. } => *code,
        }
    }

    /// The panic text: the crate, the role's name, the code, and the sentence, which names the
    /// role by its prose name.
    #[must_use]
    pub fn text(&self, role: &str, prose: &str) -> String {
        let krate = match self {
            Self::Role { krate, .. } => *krate,
            _ => "lez-authority",
        };
        format!(
            "{krate}/{role} E{}: {}",
            self.code(),
            self.describe(role, prose)
        )
    }

    /// The sentence. It names the holder by the role's prose name, and the slot by the
    /// role's name.
    #[must_use]
    pub fn describe(&self, role: &str, prose: &str) -> String {
        match self {
            Self::NotInitialized => "the config account is not initialized".into(),
            Self::ConfigDoesNotDecode => "the config account does not decode".into(),
            Self::MessageDoesNotDecode => "the message does not decode".into(),
            Self::MissingSignature => "the sender must authorize the change".into(),
            Self::NotOwnProgram => {
                "a forwarded message only comes from this program's own actor".into()
            }
            Self::ForwardedNotAtConfig => {
                "forwarded messages are only handled at the config account".into()
            }
            Self::NotThroughSeat => {
                format!("a message that needs the {prose} must go through the sender's own actor")
            }
            Self::NotInstalled => format!("there is no {prose}"),
            Self::AlreadyInstalled => format!("the {prose} is already installed"),
            Self::NotDeployer => format!(
                "only the deployer installs the first {prose}: the program's own account, through its seat at top level"
            ),
            Self::NotGenesis => {
                format!("only the genesis key installs the first {prose}, through its seat at top level")
            }
            Self::NoNomination => format!("no {prose} nominee is pending"),
            Self::NotNominee => format!("the signing account is not the nominated {prose}"),
            Self::SeedProofFailed => {
                "a nominee reached through a program must prove itself with the seed that derives it under that program".into()
            }
            Self::Renounced => format!("the {prose} is renounced"),
            Self::NotHolder => format!("the signing account is not the {prose}"),
            Self::ViaNotAllowed => {
                format!("the {prose} acts at top level, or through the program that made it the {prose}")
            }
            Self::SlotChanged => format!("a handler must not change the {role} slot"),
            Self::NotFromConfig => "this message comes only from the config account".into(),
            Self::LibrarySend => {
                "a handler must not send a library message to the config account".into()
            }
            Self::ClearanceFromOutside => {
                "a request for clearance only comes from this program's own actors".into()
            }
            Self::ClearanceNotAtConfig => {
                "requests for clearance are only handled at the config account".into()
            }
            Self::ByNominationOnly => {
                format!("the {prose} is appointed by nomination, never installed")
            }
            Self::Role { sentence, .. } => (*sentence).into(),
        }
    }
}

/// A refusal as a value: what was refused, and the role whose rule refused it. [`guard()`]
/// panics with its [`text`](Self::text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refusal {
    /// The role's name, in the text's prefix.
    pub role: &'static str,
    /// The role's prose name, in the text's sentence.
    pub prose: &'static str,
    /// What was refused.
    pub error: AuthorityError,
}

impl Refusal {
    /// `error`, refused by role `R`'s rule.
    #[must_use]
    pub fn of<R: Role>(error: AuthorityError) -> Self {
        Self {
            role: R::NAME,
            prose: R::PROSE_NAME,
            error,
        }
    }

    /// The panic text.
    #[must_use]
    pub fn text(&self) -> String {
        self.error.text(self.role, self.prose)
    }
}
