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

/// The state kept on the config account: the admin's slot and the freeze authority's.
#[derive(Debug, Clone, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, HasSlot)]
pub struct Config {
    #[slot(Admin)]
    pub admin: AuthoritySlot,
    #[slot(Freeze)]
    pub freeze: FreezeSlot,
}

/// The state the program keeps at an owner's own actor.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Balance {
    pub amount: u64,
}

/// The messages the program accepts. The admin's and the freeze authority's operations are not
/// among them: a client sends those with `admin_op` and `freeze_authority_op`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize, Gated)]
pub enum Message {
    /// The admin mints `amount` to `to`. The config credits `to`'s own actor.
    #[only(Admin)]
    Mint { to: AccountId, amount: u64 },
    /// What an owner's actor receives from the config. Nobody else may send it.
    #[from_config]
    Credit(u64),
}
