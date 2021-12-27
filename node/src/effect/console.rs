use casper_types::EraId;
use futures::Future;
use serde::Serialize;

use super::Responder;

pub(crate) struct DumpConsensusStateRequest {
    /// Era to serialize.
    ///
    /// If not given, use active era.
    pub(crate) era_id: Option<EraId>,
    pub(crate) serializer: Box<dyn FnOnce(&dyn erased_serde::Serialize) -> Vec<u8>>,
    /// Responder to send the serialized representation into.
    pub(crate) responder: Responder<Vec<u8>>,
}

impl DumpConsensusStateRequest {
    pub(crate) fn answer<T: Serialize>(self, value: &T) -> impl Future<Output = ()> {
        let buf = (self.serializer)(value);

        self.responder.respond(buf)
    }
}
