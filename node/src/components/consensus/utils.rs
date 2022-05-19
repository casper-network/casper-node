use std::collections::{BTreeMap, HashSet};

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
    let mut minimum_weight: Option<U512> = None;
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
                if minimum_weight.map_or(true, |min_w| *validator_weight < min_w) {
                    minimum_weight = Some(*validator_weight);
                }
                signature_weight += *validator_weight;
            }
        }
    }

    let weight_minus_minimum = signature_weight - minimum_weight.unwrap_or_else(U512::zero);

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

    // Verify: the total weight decreased by the smallest weight of a single signature should be
    // below threshold (anti-spam countermeasure).
    if weight_minus_minimum * U512::from(*lower_bound.denom())
        > total_weight * U512::from(*lower_bound.numer())
    {
        return Err(FinalitySignatureError::TooManySignatures {
            trusted_validator_weights: trusted_validator_weights.clone(),
            block_signatures: Box::new(block_signatures.clone()),
            signature_weight: Box::new(signature_weight),
            weight_minus_minimum: Box::new(weight_minus_minimum),
            total_validator_weight: Box::new(total_weight),
            finality_threshold_fraction,
        });
    }

    Ok(())
}

pub(crate) fn get_minimal_set_of_signatures(
    trusted_validator_weights: &BTreeMap<PublicKey, U512>,
    finality_threshold_fraction: Ratio<u64>,
    mut block_signatures: BlockSignatures,
) -> BlockSignatures {
    // Calculate the values for comparison.
    let total_weight: U512 = trusted_validator_weights
        .iter()
        .map(|(_, weight)| *weight)
        .sum();

    let lower_bound = (finality_threshold_fraction + 1) / 2;

    let mut sig_weights: Vec<_> = block_signatures
        .proofs
        .keys()
        .filter_map(|pub_key| {
            trusted_validator_weights
                .get(pub_key)
                .map(|weight| (pub_key.clone(), *weight))
        })
        .collect();
    // Sort descending by weight.
    sig_weights.sort_unstable_by(|a, b| b.1.cmp(&a.1));

    let mut accumulated_weight = U512::zero();
    let keys_to_retain: HashSet<_> = sig_weights
        .into_iter()
        .take_while(|(_, weight)| {
            let to_take = accumulated_weight * U512::from(*lower_bound.denom())
                <= total_weight * U512::from(*lower_bound.numer());
            accumulated_weight += *weight;
            to_take
        })
        .map(|(pub_key, _)| pub_key)
        .collect();

    block_signatures
        .proofs
        .retain(|pub_key, _| keys_to_retain.contains(pub_key));

    block_signatures
}
