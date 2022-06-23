use std::collections::{BTreeMap, HashSet};

use casper_types::{PublicKey, U512};
use num::rational::Ratio;

use crate::{components::consensus::error::FinalitySignatureError, types::BlockSignatures};

/// Computes the quorum for the fraction of weight of signatures that will be considered
/// sufficient.
fn quorum_fraction(finality_threshold_fraction: Ratio<u64>) -> Ratio<u64> {
    (finality_threshold_fraction + 1) / 2
}

/// Returns `Ok(())` if the finality signatures' total weight exceeds the threshold which is
/// calculated using the provided quorum formula. Returns an error if it doesn't, or if one of the
/// signatures does not belong to a validator.
///
/// This does _not_ cryptographically verify the signatures.
pub(crate) fn check_sufficient_finality_signatures_with_quorum_formula<F>(
    trusted_validator_weights: &BTreeMap<PublicKey, U512>,
    finality_threshold_fraction: Ratio<u64>,
    block_signatures: &BlockSignatures,
    quorum_formula: F,
) -> Result<(), FinalitySignatureError>
where
    F: Fn(Ratio<u64>) -> Ratio<u64>,
{
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
                });
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

    let quorum_fraction = (quorum_formula)(finality_threshold_fraction);
    // Verify: signature_weight / total_weight >= lower_bound
    // Equivalent to the following
    if signature_weight * U512::from(*quorum_fraction.denom())
        <= total_weight * U512::from(*quorum_fraction.numer())
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
    if weight_minus_minimum * U512::from(*quorum_fraction.denom())
        > total_weight * U512::from(*quorum_fraction.numer())
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

/// Returns `Ok(())` if the finality signatures' total weight exceeds the threshold. Returns an
/// error if it doesn't, or if one of the signatures does not belong to a validator.
///
/// This does _not_ cryptographically verify the signatures.
pub(crate) fn check_sufficient_finality_signatures(
    trusted_validator_weights: &BTreeMap<PublicKey, U512>,
    finality_threshold_fraction: Ratio<u64>,
    block_signatures: &BlockSignatures,
) -> Result<(), FinalitySignatureError> {
    check_sufficient_finality_signatures_with_quorum_formula(
        trusted_validator_weights,
        finality_threshold_fraction,
        block_signatures,
        quorum_fraction,
    )
}

pub(crate) fn get_minimal_set_of_signatures(
    trusted_validator_weights: &BTreeMap<PublicKey, U512>,
    finality_threshold_fraction: Ratio<u64>,
    mut block_signatures: BlockSignatures,
) -> Option<BlockSignatures> {
    // Calculate the values for comparison.
    let total_weight: U512 = trusted_validator_weights
        .iter()
        .map(|(_, weight)| *weight)
        .sum();

    let lower_bound = quorum_fraction(finality_threshold_fraction);

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
            if to_take {
                accumulated_weight += *weight;
            }
            to_take
        })
        .map(|(pub_key, _)| pub_key)
        .collect();

    // Chack if we managed to collect sufficient weight (there might not have been enough
    // signatures in the first place).
    if accumulated_weight * U512::from(*lower_bound.denom())
        > total_weight * U512::from(*lower_bound.numer())
    {
        block_signatures
            .proofs
            .retain(|pub_key, _| keys_to_retain.contains(pub_key));

        Some(block_signatures)
    } else {
        // Return `None` if the signatures weren't enough.
        None
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use num::rational::Ratio;
    use rand::Rng;

    use casper_types::{
        crypto::{generate_ed25519_keypair, sign},
        testing::TestRng,
        EraId, PublicKey, SecretKey, U512,
    };

    use crate::{
        components::consensus::{
            check_sufficient_finality_signatures_with_quorum_formula, error::FinalitySignatureError,
        },
        types::{BlockHash, BlockSignatures},
    };

    use super::{check_sufficient_finality_signatures, get_minimal_set_of_signatures};

    fn generate_validators(
        n_validators: usize,
    ) -> (BTreeMap<PublicKey, SecretKey>, BTreeMap<PublicKey, U512>) {
        let mut keys = BTreeMap::new();
        let mut weights = BTreeMap::new();

        for _ in 0..n_validators {
            let (secret_key, pub_key) = generate_ed25519_keypair();
            keys.insert(pub_key.clone(), secret_key);
            weights.insert(pub_key, U512::from(1));
        }

        (keys, weights)
    }

    fn create_signatures(
        rng: &mut TestRng,
        validators: &BTreeMap<PublicKey, SecretKey>,
        n_sigs: usize,
    ) -> BlockSignatures {
        let era = rng.gen_range(10..100);

        let block_hash = BlockHash::random(rng);

        let mut sigs = BlockSignatures::new(block_hash, EraId::from(era));

        for (pub_key, secret_key) in validators.iter().take(n_sigs) {
            let sig = sign(block_hash, secret_key, pub_key);
            sigs.insert_proof(pub_key.clone(), sig);
        }

        sigs
    }

    fn test_number_of_validators(
        rng: &mut TestRng,
        n_validators: usize,
        ftt: Ratio<u64>,
        threshold: usize,
    ) {
        let (keys, weights) = generate_validators(n_validators);

        // If the initial set has too few signatures, the result should be `None`.
        let sigs = create_signatures(rng, &keys, threshold.saturating_sub(1));
        assert!(check_sufficient_finality_signatures(&weights, ftt, &sigs).is_err());
        let minimal_set = get_minimal_set_of_signatures(&weights, ftt, sigs);
        assert!(minimal_set.is_none());

        // If there were enough signatures, we should get the set with the amount equal to the
        // threshold.
        let sigs = create_signatures(rng, &keys, threshold);
        let minimal_set = get_minimal_set_of_signatures(&weights, ftt, sigs);
        assert!(minimal_set.is_some());
        let minimal_set = minimal_set.unwrap();
        assert_eq!(minimal_set.proofs.len(), threshold);
        assert!(check_sufficient_finality_signatures(&weights, ftt, &minimal_set).is_ok());

        // Same if we were over the threshold initially.
        let sigs = create_signatures(rng, &keys, threshold.saturating_add(1));
        let minimal_set = get_minimal_set_of_signatures(&weights, ftt, sigs);
        assert!(minimal_set.is_some());
        let minimal_set = minimal_set.unwrap();
        assert_eq!(minimal_set.proofs.len(), threshold);
        assert!(check_sufficient_finality_signatures(&weights, ftt, &minimal_set).is_ok());
    }

    #[test]
    fn should_generate_minimal_set_correctly() {
        let mut rng = TestRng::new();

        let ftt = Ratio::new(1, 3);

        test_number_of_validators(&mut rng, 8, ftt, 6);
        test_number_of_validators(&mut rng, 9, ftt, 7);
        test_number_of_validators(&mut rng, 10, ftt, 7);
    }

    #[test]
    fn finality_signatures_sufficiency() {
        let mut rng = TestRng::new();

        // Total validator weights is 12 (1 for each validator).
        let (validators, validator_weights) = generate_validators(20);

        let finality_threshold_fraction = Ratio::new_raw(1, 3);

        // for 20 validators with 20 total validator weight,
        //   and `finality_threshold_fraction` = 1/3 (~=  6.666)
        //   and the `quorum fraction` = 2/3         (~= 13.333)
        //
        // we need signaturess of weight:
        //   - 13 or less for `InsufficientWeightForFinality`
        //   - 14 for Ok
        //   - 15 or more for `TooManySignatures`

        let insufficient = create_signatures(&mut rng, &validators, 13);
        let just_enough_weight = create_signatures(&mut rng, &validators, 14);
        let too_many = create_signatures(&mut rng, &validators, 15);

        let result = check_sufficient_finality_signatures(
            &validator_weights,
            finality_threshold_fraction,
            &insufficient,
        );
        assert!(matches!(
            result,
            Err(FinalitySignatureError::InsufficientWeightForFinality { .. })
        ));

        let result = check_sufficient_finality_signatures(
            &validator_weights,
            finality_threshold_fraction,
            &just_enough_weight,
        );
        assert!(result.is_ok());

        let result = check_sufficient_finality_signatures(
            &validator_weights,
            finality_threshold_fraction,
            &too_many,
        );
        assert!(matches!(
            result,
            Err(FinalitySignatureError::TooManySignatures { .. })
        ));
    }

    #[test]
    fn finality_signatures_sufficiency_with_quorum_formula() {
        let mut rng = TestRng::new();

        // Total validator weights is 12 (1 for each validator).
        let (validators, validator_weights) = generate_validators(20);

        let finality_threshold_fraction = Ratio::new_raw(1, 3);

        // `identity` function is transparent, so the calculated quorum fraction will be equal to
        // the `finality_threshold_fraction`.
        let custom_quorum_formula = std::convert::identity;

        // for 20 validators with 20 total validator weight,
        //   and `finality_threshold_fraction` = 1/3 (~=  6.666)
        //   and the `quorum fraction` = 1/3         (~=  6.666)
        //
        // we need signaturess of weight:
        //   - 6 or less for `InsufficientWeightForFinality`
        //   - 7 for Ok
        //   - 8 or more for `TooManySignatures`

        let insufficient = create_signatures(&mut rng, &validators, 6);
        let just_enough_weight = create_signatures(&mut rng, &validators, 7);
        let too_many = create_signatures(&mut rng, &validators, 8);

        let result = check_sufficient_finality_signatures_with_quorum_formula(
            &validator_weights,
            finality_threshold_fraction,
            &insufficient,
            custom_quorum_formula,
        );
        assert!(matches!(
            result,
            Err(FinalitySignatureError::InsufficientWeightForFinality { .. })
        ));

        let result = check_sufficient_finality_signatures_with_quorum_formula(
            &validator_weights,
            finality_threshold_fraction,
            &just_enough_weight,
            custom_quorum_formula,
        );
        assert!(result.is_ok());

        let result = check_sufficient_finality_signatures_with_quorum_formula(
            &validator_weights,
            finality_threshold_fraction,
            &too_many,
            custom_quorum_formula,
        );
        assert!(matches!(
            result,
            Err(FinalitySignatureError::TooManySignatures { .. })
        ));
    }

    #[test]
    fn detects_bogus_validator() {
        let mut rng = TestRng::new();

        let (validators, validator_weights) = generate_validators(20);
        let finality_threshold_fraction = Ratio::new_raw(1, 3);

        // Generate correct signatures.
        let mut signatures = create_signatures(&mut rng, &validators, 14);
        let result = check_sufficient_finality_signatures(
            &validator_weights,
            finality_threshold_fraction,
            &signatures,
        );
        assert!(result.is_ok());

        // Smuggle a bogus proof in.
        let (_, pub_key) = generate_ed25519_keypair();
        signatures.insert_proof(pub_key, *signatures.proofs.iter().next().unwrap().1);
        let result = check_sufficient_finality_signatures(
            &validator_weights,
            finality_threshold_fraction,
            &signatures,
        );
        assert!(matches!(
            result,
            Err(FinalitySignatureError::BogusValidator { .. })
        ));
    }
}
