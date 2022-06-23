use std::{
    collections::HashSet,
    fmt::{self, Display, Formatter},
};

use serde::Serialize;

use casper_execution_engine::core::engine_state::{self, genesis::GenesisSuccess, UpgradeSuccess};
use casper_types::PublicKey;

use super::{Error, FastSyncOutcome};
use crate::{
    contract_runtime::{BlockAndExecutionEffects, BlockExecutionError},
    types::{ActivationPoint, BlockHeader},
};

#[derive(Debug, Serialize)]
pub(crate) enum Event {
    /// The result of running the fast sync task.
    FastSyncResult(Result<FastSyncOutcome, Error>),
    /// The result of contract runtime running the genesis process.
    CommitGenesisResult(#[serde(skip_serializing)] Result<GenesisSuccess, engine_state::Error>),
    /// The result of contract runtime running the upgrade process.
    UpgradeResult {
        switch_block_header_before_upgrade: BlockHeader,
        is_emergency_upgrade: bool,
        #[serde(skip_serializing)]
        result: Result<UpgradeSuccess, engine_state::Error>,
    },
    /// The result of executing a finalized block.
    ExecuteImmediateSwitchBlockResult {
        maybe_switch_block_header_before_upgrade: Option<BlockHeader>,
        is_emergency_upgrade: bool,
        #[serde(skip_serializing)]
        result: Result<BlockAndExecutionEffects, BlockExecutionError>,
    },
    /// The result of running the fast sync task again after an emergency upgrade.
    FastSyncAfterEmergencyUpgradeResult {
        #[serde(skip_serializing)]
        immediate_switch_block_and_exec_effects: BlockAndExecutionEffects,
        validators_to_sign_immediate_switch_block: HashSet<PublicKey>,
        result: Result<FastSyncOutcome, Error>,
    },
    /// A new upgrade activation point was announced.
    GotUpgradeActivationPoint(ActivationPoint),
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::FastSyncResult(result) => {
                write!(formatter, "fast sync result: {:?}", result)
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
            Event::FastSyncAfterEmergencyUpgradeResult {
                result: fast_sync_result,
                ..
            } => {
                write!(
                    formatter,
                    "fast sync after emergency upgrade result: {:?}",
                    fast_sync_result
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
