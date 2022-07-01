use casper_types::{PublicKey, U512};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub transfers: Vec<Transfer>,
    pub accounts: Vec<AccountConfig>,
    #[serde(default)]
    pub only_listed_validators: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transfer {
    pub from: PublicKey,
    pub to: PublicKey,
    pub amount: U512,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountConfig {
    pub public_key: PublicKey,
    pub balance: Option<U512>,
    pub stake: Option<U512>,
}
