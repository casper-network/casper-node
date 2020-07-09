//! A store for persisting [`ProtocolData`](contract::value::ProtocolVersion) values at their
//! protocol versions.
use types::ProtocolVersion;

pub mod in_memory;
pub mod lmdb;
#[cfg(test)]
mod tests;

use crate::components::contract_runtime::storage::{protocol_data::ProtocolData, store::Store};

const NAME: &str = "PROTOCOL_DATA_STORE";

/// An entity which persists [`ProtocolData`] values at their protocol versions.
pub trait ProtocolDataStore: Store<ProtocolVersion, ProtocolData> {}
