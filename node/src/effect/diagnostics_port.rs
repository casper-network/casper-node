use std::{
    borrow::Cow,
    fmt::{Debug, Display},
};

use casper_types::EraId;
use datasize::DataSize;
use futures::Future;
use serde::Serialize;

use super::Responder;
use crate::components::consensus::EraDump;

/// A request to dump the internal consensus state of a specific era.
#[derive(DataSize, Serialize)]
pub(crate) struct DumpConsensusStateRequest {
    /// Era to serialize.
    ///
    /// If not given, use active era.
    pub(crate) era_id: Option<EraId>,
    /// Serialization function to serialize the given era with.
    #[data_size(skip)]
    #[serde(skip)]
    pub(crate) serialize: fn(&EraDump) -> Result<Vec<u8>, Cow<'static, str>>,
    /// Responder to send the serialized representation into.
    pub(crate) responder: Responder<Result<Vec<u8>, Cow<'static, str>>>,
}

impl DumpConsensusStateRequest {
    pub(crate) fn answer(
        self,
        value: Result<&EraDump, Cow<'static, str>>,
    ) -> impl Future<Output = ()> {
        let answer = match value {
            Ok(data) => (self.serialize)(data),
            Err(err) => Err(err),
        };

        self.responder.respond(answer)
    }
}

impl Display for DumpConsensusStateRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "dump consensus state for ")?;
        if let Some(ref era_id) = self.era_id {
            Display::fmt(era_id, f)
        } else {
            f.write_str(" latest era")
        }
    }
}

impl Debug for DumpConsensusStateRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DumpConsensusStateRequest")
            .field("era_id", &self.era_id)
            .finish_non_exhaustive()
    }
}
