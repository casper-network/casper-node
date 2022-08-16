use std::fmt::{self, Display, Formatter};

use derive_more::From;
use serde::Serialize;

use super::Error;
use crate::{
    components::chainspec_loader::ImmediateSwitchBlockData, effect::requests::NodeStateRequest,
    types::BlockHeader,
};

#[derive(Debug, Serialize, From)]
#[allow(clippy::enum_variant_names)]
pub(crate) enum Event {
    /// The result of running the fast sync task.
    FastSyncResult {
        result: Box<Result<BlockHeader, Error>>,
        #[serde(skip_serializing)]
        maybe_immediate_switch_block_data: Option<Box<ImmediateSwitchBlockData>>,
    },
    /// A request to provide the node state.
    #[from]
    GetNodeState(NodeStateRequest),
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::FastSyncResult {
                result,
                maybe_immediate_switch_block_data,
            } => {
                write!(
                    formatter,
                    "fast sync result: {:?}; immediate switch block data: {:?}",
                    result, maybe_immediate_switch_block_data
                )
            }
            Event::GetNodeState(_) => write!(formatter, "get node state"),
        }
    }
}
