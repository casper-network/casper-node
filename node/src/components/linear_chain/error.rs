use std::{collections::BTreeMap, fmt::Debug};

use num::rational::Ratio;
use serde::Serialize;
use thiserror::Error;

use casper_execution_engine::core::engine_state::GetEraValidatorsError;
use casper_types::{EraId, PublicKey, U512};

use crate::types::{BlockHeader, BlockSignatures};

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

#[derive(Error, Debug)]
pub(crate) enum BlockSignatureError {
    #[error(
        "Block signatures contain bogus validator. \
         trusted validator weights: {trusted_validator_weights:?}, \
         block signatures: {block_signatures:?}, \
         bogus validator public keys: {bogus_validators:?}"
    )]
    BogusValidators {
        trusted_validator_weights: BTreeMap<PublicKey, U512>,
        block_signatures: Box<BlockSignatures>,
        bogus_validators: Box<Vec<PublicKey>>,
    },

    #[error(
        "Insufficient weight for finality. \
         trusted validator weights: {trusted_validator_weights:?}, \
         block signatures: {block_signatures:?}, \
         signature weight: {signature_weight:?}, \
         total validator weight: {total_validator_weight}, \
         fault tolerance fraction: {fault_tolerance_fraction}"
    )]
    InsufficientWeightForFinality {
        trusted_validator_weights: BTreeMap<PublicKey, U512>,
        block_signatures: Option<Box<BlockSignatures>>,
        signature_weight: Option<Box<U512>>,
        total_validator_weight: Box<U512>,
        fault_tolerance_fraction: Ratio<u64>,
    },
}
