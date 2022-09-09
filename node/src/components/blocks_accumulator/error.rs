use thiserror::Error;

use casper_types::{crypto, EraId};

use crate::types::{BlockAddedValidationError, FinalitySignature};

#[derive(Error, Debug)]
pub(super) enum Error {
    #[error(transparent)]
    InvalidBlockAdded(BlockAddedValidationError),
    #[error(transparent)]
    InvalidFinalitySignature(crypto::Error),
    #[error("finality signature {finality_signature} for wrong era, correct era {correct_era}")]
    FinalitySignatureWithWrongEra {
        finality_signature: FinalitySignature,
        correct_era: EraId,
    },
}
