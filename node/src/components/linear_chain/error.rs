use std::fmt::Debug;

use serde::Serialize;
use thiserror::Error;

use casper_execution_engine::core::engine_state::GetEraValidatorsError;
use casper_types::EraId;

use crate::types::BlockHeader;

#[derive(Error, Debug, Serialize)]
pub(crate) enum Error {
    #[error("cannot get switch block for era {era_id}")]
    NoSwitchBlockForEra { era_id: EraId },

    #[error("switch block at height {height} for era {era_id} contains no validator weights")]
    MissingNextEraValidators { height: u64, era_id: EraId },

    /// Error getting era validators from the execution engine.
    #[error(transparent)]
    GetEraValidators(
        #[from]
        #[serde(skip_serializing)]
        GetEraValidatorsError,
    ),

    /// Expected entry in validator map not found.
    #[error("stored block has no validator map entry for era {missing_era_id}: {block_header:?}")]
    MissingValidatorMapEntry {
        block_header: Box<BlockHeader>,
        missing_era_id: EraId,
    },
}
