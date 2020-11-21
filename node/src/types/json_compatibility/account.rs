// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use datasize::DataSize;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use casper_execution_engine::shared::account::Account as ExecutionEngineAccount;
use casper_types::{account::AccountHash, URef};

use super::NamedKey;

#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize, DataSize, JsonSchema)]
struct AssociatedKey {
    account_hash: AccountHash,
    weight: u8,
}

/// Thresholds that have to be met when executing an action of a certain type.
#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize, DataSize, JsonSchema)]
struct ActionThresholds {
    deployment: u8,
    key_management: u8,
}

/// Structure representing a user's account, stored in global state.
#[derive(PartialEq, Eq, Clone, Debug, Serialize, Deserialize, DataSize, JsonSchema)]
pub struct Account {
    account_hash: AccountHash,
    named_keys: Vec<NamedKey>,
    #[data_size(skip)]
    main_purse: URef,
    associated_keys: Vec<AssociatedKey>,
    action_thresholds: ActionThresholds,
}

impl From<&ExecutionEngineAccount> for Account {
    fn from(ee_account: &ExecutionEngineAccount) -> Self {
        Account {
            account_hash: ee_account.account_hash(),
            named_keys: ee_account
                .named_keys()
                .iter()
                .map(|(name, key)| NamedKey {
                    name: name.clone(),
                    key: key.to_formatted_string(),
                })
                .collect(),
            main_purse: ee_account.main_purse(),
            associated_keys: ee_account
                .associated_keys()
                .map(|(account_hash, weight)| AssociatedKey {
                    account_hash: *account_hash,
                    weight: weight.value(),
                })
                .collect(),
            action_thresholds: ActionThresholds {
                deployment: ee_account.action_thresholds().deployment().value(),
                key_management: ee_account.action_thresholds().key_management().value(),
            },
        }
    }
}
