//! TODO REMOVE
//! A store for persisting `ProtocolData` values at their
//! protocol versions.

use casper_types::ProtocolVersion;

/// TODO REMOVE
pub mod in_memory;
/// TODO REMOVE
pub mod lmdb;
#[cfg(test)]
mod tests;

use crate::storage::{protocol_data::ProtocolData, store::Store};

const NAME: &str = "PROTOCOL_DATA_STORE";

/// An entity which persists `ProtocolData` values at their protocol versions.
pub trait ProtocolDataStore: Store<ProtocolVersion, ProtocolData> {}
