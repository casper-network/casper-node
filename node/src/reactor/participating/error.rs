use thiserror::Error;

use crate::{
    components::{contract_runtime, contract_runtime::BlockExecutionError, small_network, storage},
    types::{Block, BlockHeader},
    utils::ListeningError,
};
use casper_execution_engine::core::engine_state;
use casper_types::{bytesrepr, EraId, ProtocolVersion};

/// Error type returned by the validator reactor.
#[derive(Debug, Error)]
pub(crate) enum Error {
    /// Metrics-related error
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),

    /// `SmallNetwork` component error.
    #[error("small network error: {0}")]
    SmallNetwork(#[from] small_network::Error),

    /// An error starting one of the HTTP servers.
    #[error("http server listening error: {0}")]
    HttpServerListening(#[from] ListeningError),

    /// `Storage` component error.
    #[error("storage error: {0}")]
    Storage(#[from] storage::Error),

    /// `Consensus` component error.
    #[error("consensus error: {0}")]
    Consensus(#[from] anyhow::Error),

    /// `ContractRuntime` component error.
    #[error("contract runtime config error: {0}")]
    ContractRuntime(#[from] contract_runtime::ConfigError),

    /// Block execution error.
    #[error(transparent)]
    BlockExecution(#[from] BlockExecutionError),

    /// Engine state error.
    #[error(transparent)]
    EngineState(#[from] engine_state::Error),

    /// [`bytesrepr`] error.
    #[error("bytesrepr error: {0}")]
    BytesRepr(bytesrepr::Error),

    /// Cannot run genesis on a non v1.0.0 blockchain.
    #[error(
        "Genesis must run on protocol version 1.0.0. \
         Chainspec protocol version: {chainspec_protocol_version:?}"
    )]
    GenesisNeedsProtocolVersion1_0_0 {
        chainspec_protocol_version: ProtocolVersion,
    },

    /// Cannot run genesis on preexisting blockchain.
    #[error("Cannot run genesis on preexisting blockchain. First block: {highest_block_header:?}")]
    CannotRunGenesisOnPreExistingBlockchain {
        highest_block_header: Box<BlockHeader>,
    },

    /// No such switch block for upgrade era.
    #[error("No such switch block for upgrade era: {upgrade_era_id}")]
    NoSuchSwitchBlockHeaderForUpgradeEra {
        /// The upgrade era id.
        upgrade_era_id: EraId,
    },

    /// Non-emergency upgrade will clobber existing blockchain.
    #[error(
        "Non-emergency upgrade will clobber existing blockchain. \
         Preexisting block header: {preexisting_block_header}"
    )]
    NonEmergencyUpgradeWillClobberExistingBlockChain {
        /// A preexisting block header.
        preexisting_block_header: Box<BlockHeader>,
    },

    /// Failed to create a switch block immediately after genesis or upgrade.
    #[error(
        "Failed to create a switch block immediately after genesis or upgrade. \
         New bad block we made: {new_bad_block}"
    )]
    FailedToCreateSwitchBlockAfterGenesisOrUpgrade {
        /// A new block we made which should be a switch block but is not.
        new_bad_block: Box<Block>,
    },
}

impl From<bytesrepr::Error> for Error {
    fn from(err: bytesrepr::Error) -> Self {
        Self::BytesRepr(err)
    }
}
