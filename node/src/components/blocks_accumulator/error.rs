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
    #[error(
        "validator weights of era {validator_weights_era} provided for block in era {block_era}"
    )]
    WrongEraWeights {
        block_era: EraId,
        validator_weights_era: EraId,
    },
}
