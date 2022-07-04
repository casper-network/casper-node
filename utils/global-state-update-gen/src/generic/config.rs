use casper_types::{account::AccountHash, PublicKey, U512};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub transfers: Vec<Transfer>,
    #[serde(default)]
    pub accounts: Vec<AccountConfig>,
    #[serde(default)]
    pub only_listed_validators: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transfer {
    pub from: AccountHash,
    pub to: AccountHash,
    pub amount: U512,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    pub public_key: PublicKey,
    pub balance: Option<U512>,
    pub stake: Option<U512>,
}
