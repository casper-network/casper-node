use std::fmt::{self, Display, Formatter};

use derive_more::From;
use serde::Serialize;

use super::Error;
use crate::{effect::requests::NodeStateRequest, types::BlockHeader};

#[derive(Debug, Serialize, From)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum Event {
    /// The result of running the fast sync task.
    SyncToGenesisResult(Box<Result<(), Error>>),
    /// The result of running the fast sync task.
    FastSyncResult {
        result: Box<Result<BlockHeader, Error>>,
    },
    /// A request to provide the node state.
    #[from]
    GetNodeState(NodeStateRequest),
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::FastSyncResult { result } => {
                write!(formatter, "fast sync result: {:?}", result)
            }
            Event::SyncToGenesisResult(result) => {
                write!(formatter, "sync to genesis result: {:?}", result)
            }
            Event::GetNodeState(_) => write!(formatter, "get node state"),
        }
    }
}
