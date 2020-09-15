//! This file provides types to allow conversion from an EE `Account` into a similar type
//! which can be serialized to a valid JSON representation.
//!
//! It is stored as metadata related to a given deploy, and made available to clients via the
//! JSON-RPC API.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use casper_execution_engine::shared::account::{
    Account as ExecutionEngineAccount, ActionThresholds as ExecutionEngineActionThresholds,
};

/// Representation of a client's account.
///
/// Note that the `account_hash` and `associated_keys` members are deliberately omitted since
/// account hashes are an internal detail of the EE and should not be provided to users due to
/// confusion with the account's public key.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
pub struct Account {
    named_keys: BTreeMap<String, String>,
    main_purse: String,
    action_thresholds: ActionThresholds,
}

impl From<&ExecutionEngineAccount> for Account {
    fn from(ee_account: &ExecutionEngineAccount) -> Self {
        Account {
            named_keys: super::convert_named_keys(ee_account.named_keys()),
            main_purse: ee_account.main_purse().to_formatted_string(),
            action_thresholds: ActionThresholds::from(ee_account.action_thresholds()),
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
