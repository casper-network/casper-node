//! This file provides types to allow conversion from an EE `Account` into a similar type
//! which can be serialized to a valid JSON representation.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use casper_execution_engine::shared::account::{
    Account as ExecutionEngineAccount, ActionThresholds as ExecutionEngineActionThresholds,
};
use casper_types::account::{AccountHash, Weight};

/// Representation of a client's account.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
pub struct Account {
    account_hash: String,
    named_keys: BTreeMap<String, String>,
    main_purse: String,
    associated_keys: Vec<AssociatedKey>,
    action_thresholds: ActionThresholds,
}

impl From<&ExecutionEngineAccount> for Account {
    fn from(ee_account: &ExecutionEngineAccount) -> Self {
        Account {
            account_hash: hex::encode(ee_account.account_hash().as_bytes()),
            named_keys: super::convert_named_keys(ee_account.named_keys()),
            main_purse: ee_account.main_purse().to_formatted_string(),
            associated_keys: ee_account
                .get_associated_keys()
                .map(AssociatedKey::from)
                .collect(),
            action_thresholds: ActionThresholds::from(ee_account.action_thresholds()),
        }
    }
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
pub struct AssociatedKey {
    account_hash: String,
    weight: u8,
}

impl From<(&AccountHash, &Weight)> for AssociatedKey {
    fn from((ee_account_hash, ee_weight): (&AccountHash, &Weight)) -> Self {
        AssociatedKey {
            account_hash: hex::encode(ee_account_hash.as_bytes()),
            weight: ee_weight.value(),
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize, Deserialize, Debug)]
pub struct ActionThresholds {
    deployment: u8,
    key_management: u8,
}

impl From<&ExecutionEngineActionThresholds> for ActionThresholds {
    fn from(ee_action_thresholds: &ExecutionEngineActionThresholds) -> Self {
        ActionThresholds {
            deployment: ee_action_thresholds.deployment().value(),
            key_management: ee_action_thresholds.key_management().value(),
        }
    }
}
