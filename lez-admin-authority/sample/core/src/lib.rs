//! Types of the sample program. No guest code here, so wallets, tests and other
//! programs can depend on it.

use borsh::{BorshDeserialize, BorshSerialize};
use lee_core::{account::AccountId, program::PdaSeed};
use lez_admin_authority::{Admin, AuthoritySlot, Gated, HasSlot};

/// The seed of the config account, the program's one well-known account.
pub const CONFIG_SEED: PdaSeed = lez_admin_authority::seed(b"lez-admin-sample/config/v1");

/// The account the config lives on, for this program.
#[must_use]
pub fn config_account(program: AccountId) -> AccountId {
    lez_admin_authority::config_account(program, &CONFIG_SEED)
}

/// The state kept on the config account. The admin slot is one more field.
#[derive(Debug, Clone, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize, HasSlot)]
pub struct Config {
    pub open_value: u64,
    pub admin_value: u64,
    #[slot(Admin)]
    pub admin: AuthoritySlot,
}

/// The state the program keeps on a target account, set by the admin through the hub.
#[derive(Debug, Clone, Default, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct TargetState {
    pub value: u64,
}

/// The messages the program accepts. The admin's operations are not among them: a client
/// sends those with `lez_admin_authority::admin_op`. The guard checks the admin before the
/// handler for every message marked `#[only(Admin)]`.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize, Gated)]
pub enum Message {
    /// Anyone.
    SetOpenValue(u64),
    /// The admin only.
    #[only(Admin)]
    SetAdminValue(u64),
    /// The admin only. The config sends `TargetValue` to `target`'s own actor.
    #[only(Admin)]
    SetTargetValue { target: AccountId, value: u64 },
    /// What a target receives from the config. Nobody else may send it.
    #[from_config]
    TargetValue(u64),
}
