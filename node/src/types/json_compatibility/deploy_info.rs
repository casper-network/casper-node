//! This file provides types to allow conversion from an EE `DeployInfo` into a similar type
//! which can be serialized to a valid JSON representation.

// TODO - remove once schemars stops causing warning.
#![allow(clippy::field_reassign_with_default)]

use casper_types::{self, U512};
use hex_fmt::HexFmt;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Representation of deploy info
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug, JsonSchema)]
pub struct DeployInfo {
    deploy_hash: String,
    transfers: Vec<String>,
    from: String,
    source: String,
    gas: U512,
}

impl From<&casper_types::DeployInfo> for DeployInfo {
    fn from(types_deploy_info: &casper_types::DeployInfo) -> Self {
        let deploy_hash = format!("{}", HexFmt(types_deploy_info.deploy_hash.as_bytes()));
        let transfers = types_deploy_info
            .transfers
            .iter()
            .map(|transfer_addr| format!("transfer-{}", HexFmt(transfer_addr)))
            .collect();
        let from = types_deploy_info.from.to_formatted_string();
        let source = types_deploy_info.source.to_formatted_string();
        let gas = types_deploy_info.gas;
        DeployInfo {
            deploy_hash,
            transfers,
            from,
            source,
            gas,
        }
    }
}
