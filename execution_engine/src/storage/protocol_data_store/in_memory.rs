use casper_types::ProtocolVersion;

use crate::storage::{
    error::in_memory::Error,
    protocol_data::ProtocolData,
    protocol_data_store::{self, ProtocolDataStore},
    store::Store,
    transaction_source::in_memory::InMemoryEnvironment,
};

/// An in-memory protocol data store
pub struct InMemoryProtocolDataStore {
    maybe_name: Option<String>,
}

impl InMemoryProtocolDataStore {
    /// Constructor for `InMemoryProtocolDataStore`.
    pub(crate) fn new(_env: &InMemoryEnvironment, maybe_name: Option<&str>) -> Self {
        let name = maybe_name
            .map(|name| format!("{}-{}", protocol_data_store::NAME, name))
            .unwrap_or_else(|| String::from(protocol_data_store::NAME));
        InMemoryProtocolDataStore {
            maybe_name: Some(name),
        }
    }
}

impl Store<ProtocolVersion, ProtocolData> for InMemoryProtocolDataStore {
    type Error = Error;
    type Handle = Option<String>;

    fn handle(&self) -> Self::Handle {
        self.maybe_name.to_owned()
    }
}

impl ProtocolDataStore for InMemoryProtocolDataStore {}
