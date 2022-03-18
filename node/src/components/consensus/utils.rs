use std::collections::BTreeMap;

use casper_types::{PublicKey, U512};
use num::rational::Ratio;

use crate::{components::consensus::error::FinalitySignatureError, types::BlockSignatures};

/// Returns `Ok(())` if the finality signatures' total weight exceeds the threshold. Returns an
/// error if it doesn't, or if one of the signatures does not belong to a validator.
///
/// This does _not_ cryptographically verify the signatures.
pub(crate) fn check_sufficient_finality_signatures(
    trusted_validator_weights: &BTreeMap<PublicKey, U512>,
    finality_threshold_fraction: Ratio<u64>,
    block_signatures: &BlockSignatures,
) -> Result<(), FinalitySignatureError> {
    // Calculate the weight of the signatures
    let mut signature_weight: U512 = U512::zero();
    for (public_key, _) in block_signatures.proofs.iter() {
        match trusted_validator_weights.get(public_key) {
            None => {
                return Err(FinalitySignatureError::BogusValidator {
                    trusted_validator_weights: trusted_validator_weights.clone(),
                    block_signatures: Box::new(block_signatures.clone()),
                    bogus_validator_public_key: Box::new(public_key.clone()),
                })
            }
            Some(validator_weight) => {
                signature_weight += *validator_weight;
            }
        }
    }

    // Check the finality signatures have sufficient weight
    let total_weight: U512 = trusted_validator_weights
        .iter()
        .map(|(_, weight)| *weight)
        .sum();

    let lower_bound = (finality_threshold_fraction + 1) / 2;
    // Verify: signature_weight / total_weight >= lower_bound
    // Equivalent to the following
    if signature_weight * U512::from(*lower_bound.denom())
        <= total_weight * U512::from(*lower_bound.numer())
    {
        return Err(FinalitySignatureError::InsufficientWeightForFinality {
            trusted_validator_weights: trusted_validator_weights.clone(),
            block_signatures: Box::new(block_signatures.clone()),
            signature_weight: Box::new(signature_weight),
            total_validator_weight: Box::new(total_weight),
            finality_threshold_fraction,
        });
    }

    Ok(())
}
