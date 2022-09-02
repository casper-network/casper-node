use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use casper_types::{account::AccountHash, PublicKey, U512};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
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
    pub validator: Option<ValidatorConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ValidatorConfig {
    pub bonded_amount: U512,
    pub delegation_rate: Option<u8>,
    pub delegators: Option<Vec<DelegatorConfig>>,
}

impl ValidatorConfig {
    pub fn delegators_map(&self) -> Option<BTreeMap<PublicKey, U512>> {
        self.delegators.as_ref().map(|delegators| {
            delegators
                .iter()
                .map(|delegator| (delegator.public_key.clone(), delegator.delegated_amount))
                .collect()
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegatorConfig {
    pub public_key: PublicKey,
    pub delegated_amount: U512,
}
