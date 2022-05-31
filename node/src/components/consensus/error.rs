use std::collections::BTreeMap;

use num::rational::Ratio;
use thiserror::Error;

use casper_types::{EraId, PublicKey, U512};

use crate::types::{BlockHeader, BlockSignatures};

#[derive(Error, Debug)]
pub enum FinalitySignatureError {
    #[error(
        "Block signatures contain bogus validator. \
         trusted validator weights: {trusted_validator_weights:?}, \
         block signatures: {block_signatures:?}, \
         bogus validator public key: {bogus_validator_public_key:?}"
    )]
    BogusValidator {
        trusted_validator_weights: BTreeMap<PublicKey, U512>,
        block_signatures: Box<BlockSignatures>,
        bogus_validator_public_key: Box<PublicKey>,
    },

    #[error(
        "Insufficient weight for finality. \
         trusted validator weights: {trusted_validator_weights:?}, \
         block signatures: {block_signatures:?}, \
         signature weight: {signature_weight}, \
         total validator weight: {total_validator_weight}, \
         finality threshold fraction: {finality_threshold_fraction}"
    )]
    InsufficientWeightForFinality {
        trusted_validator_weights: BTreeMap<PublicKey, U512>,
        block_signatures: Box<BlockSignatures>,
        signature_weight: Box<U512>,
        total_validator_weight: Box<U512>,
        finality_threshold_fraction: Ratio<u64>,
    },

    #[error(
        "Anti-spam: signature set not minimal. \
         trusted validator weights: {trusted_validator_weights:?}, \
         block signatures: {block_signatures:?}, \
         signature weight: {signature_weight}, \
         weight minus minimum: {weight_minus_minimum}, \
         total validator weight: {total_validator_weight}, \
         finality threshold fraction: {finality_threshold_fraction}"
    )]
    TooManySignatures {
        trusted_validator_weights: BTreeMap<PublicKey, U512>,
        block_signatures: Box<BlockSignatures>,
        signature_weight: Box<U512>,
        weight_minus_minimum: Box<U512>,
        total_validator_weight: Box<U512>,
        finality_threshold_fraction: Ratio<u64>,
    },
}

#[derive(Error, Debug)]
pub enum CreateNewEraError {
    #[error("Attempted to create era with no switch blocks.")]
    AttemptedToCreateEraWithNoSwitchBlocks,
    #[error("Attempted to create {era_id} with non-switch block {last_block_header:?}.")]
    LastBlockHeaderNotASwitchBlock {
        era_id: EraId,
        last_block_header: Box<BlockHeader>,
    },
    #[error("Attempted to create {era_id} with too few switch blocks {switch_blocks:?}.")]
    InsufficientSwitchBlocks {
        era_id: EraId,
        switch_blocks: Vec<BlockHeader>,
    },
    #[error(
        "Attempted to create {era_id} with switch blocks from unexpected eras: {switch_blocks:?}."
    )]
    WrongSwitchBlockEra {
        era_id: EraId,
        switch_blocks: Vec<BlockHeader>,
    },
}
