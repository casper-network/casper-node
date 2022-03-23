use std::fmt::{self, Display, Formatter};

use serde::Serialize;

use casper_execution_engine::core::engine_state::{self, genesis::GenesisSuccess, UpgradeSuccess};

use super::Error;
use crate::{
    contract_runtime::{BlockAndExecutionEffects, BlockExecutionError},
    types::{ActivationPoint, BlockHash, BlockHeader},
};

#[derive(Debug, Serialize)]
pub(crate) enum Event {
    /// The result of getting the highest block from storage.
    HighestBlockHash(Option<BlockHash>),
    /// The result of the sync task.
    SyncResult(Result<BlockHeader, Error>),
    /// The result of contract runtime running the genesis process.
    CommitGenesisResult(#[serde(skip_serializing)] Result<GenesisSuccess, engine_state::Error>),
    /// The result of contract runtime running the upgrade process.
    UpgradeResult {
        upgrade_block_header: BlockHeader,
        #[serde(skip_serializing)]
        result: Result<UpgradeSuccess, engine_state::Error>,
    },
    /// The result of executing a finalized block.
    ExecuteImmediateSwitchBlockResult {
        maybe_upgrade_block_header: Option<BlockHeader>,
        #[serde(skip_serializing)]
        result: Result<BlockAndExecutionEffects, BlockExecutionError>,
    },

    /// A new upgrade activation point was announced.
    GotUpgradeActivationPoint(ActivationPoint),
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::HighestBlockHash(Some(block_hash)) => {
                write!(formatter, "highest block hash {}", block_hash)
            }
            Event::HighestBlockHash(None) => {
                write!(formatter, "no highest block hash")
            }
            Event::SyncResult(result) => {
                write!(formatter, "sync result: {:?}", result)
            }
            Event::CommitGenesisResult(result) => {
                write!(formatter, "commit genesis result: {:?}", result)
            }
            Event::UpgradeResult { result, .. } => {
                write!(formatter, "upgrade result: {:?}", result)
            }
            Event::ExecuteImmediateSwitchBlockResult { result, .. } => {
                write!(
                    formatter,
                    "execute immediate switch block result: {:?}",
                    result
                )
            }
            Event::GotUpgradeActivationPoint(activation_point) => {
                write!(
                    formatter,
                    "new upgrade activation point: {:?}",
                    activation_point
                )
            }
        }
    }
}
