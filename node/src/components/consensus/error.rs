use std::collections::BTreeMap;

use num::rational::Ratio;
use thiserror::Error;

use crate::types::BlockSignatures;
use casper_types::{PublicKey, U512};

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
}
