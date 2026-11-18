//! Types of the token sample. No guest code here, so wallets, tests and other programs can
//! depend on it.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{account::AccountId, program::PdaSeed};
use lez_freeze_authority::{Admin, AuthoritySlot, Freeze, FreezeSlot, Gated, HasSlot};

/// The seed of the config account, the program's one well-known account.
pub const CONFIG_SEED: PdaSeed = lez_freeze_authority::seed(b"lez-freeze-sample/config/v1");

/// The account the config lives on, for this program.
#[must_use]
pub fn config_account(program: AccountId) -> AccountId {
    lez_freeze_authority::config_account(program, &CONFIG_SEED)
}

/// The longest notice, in bytes. Every message at the config reads the notice, so it stays
/// short.
pub const NOTICE_MAX: usize = 64;

/// The state kept on the config account: the admin's slot, the freeze authority's, and the
/// admin's notice.
#[derive(Debug, Clone, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, HasSlot)]
pub struct Config {
    #[slot(Admin)]
    pub admin: AuthoritySlot,
    #[slot(Freeze)]
    pub freeze: FreezeSlot,
    /// What the admin last posted, such as why the program is frozen, at most [`NOTICE_MAX`]
    /// bytes.
    pub notice: String,
}

/// The state the program keeps at an owner's own actor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Balance {
    pub amount: u64,
}

/// The messages the program accepts. The admin's and the freeze authority's operations are not
/// among them: a client sends those with `admin_op`, `freeze_authority_op` and `freeze_op`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize, Gated)]
pub enum Message {
    /// The admin mints `amount` to `to`. The config credits `to`'s own actor.
    #[only(Admin)]
    Mint { to: AccountId, amount: u64 },
    /// What an owner's actor receives from another actor of the program: the config for a mint,
    /// the sender's actor for a transfer. Its handler refuses anyone else.
    Credit(u64),
    /// The admin posts a notice on the config. A freeze lets it through, so the admin can say
    /// why the program is frozen.
    #[only(Admin)]
    #[exempt(Freeze)]
    SetNotice(String),
    /// An owner moves `amount` to `to`. Sent to the owner's own actor by the transaction itself,
    /// not relayed by another actor, and signed by the owner: that actor debits the owner and
    /// credits `to`'s actor. A program running in a transaction the owner signed cannot move the
    /// owner's balance. An owner that cannot sign, such as a PDA, is credited but never sends.
    /// The credit is a sync message, so a private recipient must be an account the sender's
    /// wallet proves for, and a transfer to another owner's private account cannot be built.
    Transfer { to: AccountId, amount: u64 },
}
