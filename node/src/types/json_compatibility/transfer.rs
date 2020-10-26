//! This file provides types to allow conversion from an EE `Transfer` into a similar type
//! which can be serialized to a valid JSON representation.

use casper_types::U512;
use hex_fmt::HexFmt;
use serde::{Deserialize, Serialize};

/// Representation of a transfer.
#[derive(Clone, Eq, PartialEq, Serialize, Deserialize, Debug)]
pub struct Transfer {
    deploy_hash: String,
    from: String,
    source: String,
    target: String,
    amount: U512,
    gas: U512,
}

impl From<&casper_types::Transfer> for Transfer {
    fn from(types_transfer: &casper_types::Transfer) -> Self {
        let deploy_hash = format!("{}", HexFmt(types_transfer.deploy_hash));
        let from = types_transfer.from.to_formatted_string();
        let source = types_transfer.source.to_formatted_string();
        let target = types_transfer.target.to_formatted_string();
        let amount = types_transfer.amount;
        let gas = types_transfer.gas;
        Transfer {
            deploy_hash,
            from,
            source,
            target,
            amount,
            gas,
        }
    }
}
