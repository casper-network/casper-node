use std::collections::{BTreeMap, HashSet};

use num::rational::Ratio;

use casper_types::{PublicKey, U512};

use super::error::BlockSignatureError;
use crate::types::BlockSignatures;

/// Validates the signatures by checking if public keys in proofs matches the validators public key.
pub(crate) fn validate_block_signatures(
    signatures: &BlockSignatures,
    validator_weights: &BTreeMap<PublicKey, U512>,
) -> Result<(), BlockSignatureError> {
    let mut bogus_validators = vec![];
    for (public_key, _) in signatures.proofs.iter() {
        if validator_weights.get(public_key).is_none() {
            bogus_validators.push(public_key.clone());
        }
    }

    if !bogus_validators.is_empty() {
        return Err(BlockSignatureError::BogusValidators {
            trusted_validator_weights: validator_weights.clone(),
            block_signatures: Box::new(signatures.clone()),
            bogus_validators: Box::new(bogus_validators),
        });
    }

    Ok(())
}

/// Computes the quorum for the fraction of weight of signatures that will be considered
/// sufficient. This is the lowest weight so that any two sets of validators with that weight have
/// at least one honest validator in common.
fn quorum_fraction(fault_tolerance_fraction: Ratio<u64>) -> Ratio<u64> {
    (fault_tolerance_fraction + 1) / 2
}

/// Returns `Ok(())` if the block signatures' total weight exceeds the threshold which is
/// calculated using the provided quorum formula. Returns an error if it doesn't, or if one of the
/// signatures does not belong to a validator.
///
/// This does _not_ cryptographically verify the signatures.
pub(crate) fn check_sufficient_block_signatures_with_quorum_formula<F>(
    trusted_validator_weights: &BTreeMap<PublicKey, U512>,
    fault_tolerance_fraction: Ratio<u64>,
    maybe_block_signatures: Option<&BlockSignatures>,
    quorum_formula: F,
) -> Result<(), BlockSignatureError>
where
    F: Fn(Ratio<u64>) -> Ratio<u64>,
{
    // Calculate the weight of the signatures
    let mut signature_weight: U512 = U512::zero();
    let mut minimum_weight: Option<U512> = None;

    let total_weight: U512 = trusted_validator_weights
        .iter()
        .map(|(_, weight)| *weight)
        .sum();

    match maybe_block_signatures {
        Some(block_signatures) => {
            let mut bogus_validators = vec![];
            for (public_key, _) in block_signatures.proofs.iter() {
                match trusted_validator_weights.get(public_key) {
                    None => {
                        bogus_validators.push(public_key.clone());
                        continue;
                    }
                    Some(validator_weight) => {
                        if minimum_weight.map_or(true, |min_w| *validator_weight < min_w) {
                            minimum_weight = Some(*validator_weight);
                        }
                        signature_weight += *validator_weight;
                    }
                }
            }
            if !bogus_validators.is_empty() {
                return Err(BlockSignatureError::BogusValidators {
                    trusted_validator_weights: trusted_validator_weights.clone(),
                    block_signatures: Box::new(block_signatures.clone()),
                    bogus_validators: Box::new(bogus_validators),
                });
            }
            let weight_minus_minimum = signature_weight - minimum_weight.unwrap_or_else(U512::zero);

            let quorum_fraction = (quorum_formula)(fault_tolerance_fraction);
            // Verify: signature_weight / total_weight >= lower_bound
            // Equivalent to the following
            if signature_weight * U512::from(*quorum_fraction.denom())
                <= total_weight * U512::from(*quorum_fraction.numer())
            {
                return Err(BlockSignatureError::InsufficientWeightForFinality {
                    trusted_validator_weights: trusted_validator_weights.clone(),
                    block_signatures: maybe_block_signatures
                        .map(|signatures| Box::new(signatures.clone())),
                    signature_weight: Some(Box::new(signature_weight)),
                    total_validator_weight: Box::new(total_weight),
                    fault_tolerance_fraction,
                });
            }

            Ok(())
        }
        None => {
            // No signatures provided, return early.
            Err(BlockSignatureError::InsufficientWeightForFinality {
                trusted_validator_weights: trusted_validator_weights.clone(),
                block_signatures: None,
                signature_weight: None,
                total_validator_weight: Box::new(total_weight),
                fault_tolerance_fraction,
            })
        }
    }
}

/// Returns `Ok(())` if the block signatures' total weight exceeds the threshold calculated by
/// the [quorum_fraction] function. Returns an error if it doesn't, or if one of the signatures does
/// not belong to a validator.
///
/// This does _not_ cryptographically verify the signatures.
pub(crate) fn check_sufficient_block_signatures(
    trusted_validator_weights: &BTreeMap<PublicKey, U512>,
    fault_tolerance_fraction: Ratio<u64>,
    block_signatures: Option<&BlockSignatures>,
) -> Result<(), BlockSignatureError> {
    check_sufficient_block_signatures_with_quorum_formula(
        trusted_validator_weights,
        fault_tolerance_fraction,
        block_signatures,
        quorum_fraction,
    )
}

// TODO - remove the `allow` below.
#[allow(unused)]
pub(crate) fn get_minimal_set_of_block_signatures(
    trusted_validator_weights: &BTreeMap<PublicKey, U512>,
    fault_tolerance_fraction: Ratio<u64>,
    mut block_signatures: BlockSignatures,
) -> Option<BlockSignatures> {
    // Calculate the values for comparison.
    let total_weight: U512 = trusted_validator_weights
        .iter()
        .map(|(_, weight)| *weight)
        .sum();

    let lower_bound = quorum_fraction(fault_tolerance_fraction);

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

    // Check if we managed to collect sufficient weight (there might not have been enough
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
    use rand::Rng;

    use casper_types::{crypto, testing::TestRng, EraId, SecretKey};

    use super::*;
    use crate::types::BlockHash;

    const TEST_VALIDATOR_WEIGHT: usize = 1;

    fn generate_validators(
        n_validators: usize,
    ) -> (BTreeMap<PublicKey, SecretKey>, BTreeMap<PublicKey, U512>) {
        let mut keys = BTreeMap::new();
        let mut weights = BTreeMap::new();

        for _ in 0..n_validators {
            let (secret_key, pub_key) = crypto::generate_ed25519_keypair();
            keys.insert(pub_key.clone(), secret_key);
            weights.insert(pub_key, U512::from(TEST_VALIDATOR_WEIGHT));
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
            let sig = crypto::sign(block_hash, secret_key, pub_key);
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
        assert!(check_sufficient_block_signatures(&weights, ftt, Some(&sigs)).is_err());
        let minimal_set = get_minimal_set_of_block_signatures(&weights, ftt, sigs);
        assert!(minimal_set.is_none());

        // If there were enough signatures, we should get the set with the amount equal to the
        // threshold.
        let sigs = create_signatures(rng, &keys, threshold);
        let minimal_set = get_minimal_set_of_block_signatures(&weights, ftt, sigs);
        assert!(minimal_set.is_some());
        let minimal_set = minimal_set.unwrap();
        assert_eq!(minimal_set.proofs.len(), threshold);
        assert!(check_sufficient_block_signatures(&weights, ftt, Some(&minimal_set)).is_ok());

        // Same if we were over the threshold initially.
        let sigs = create_signatures(rng, &keys, threshold.saturating_add(1));
        let minimal_set = get_minimal_set_of_block_signatures(&weights, ftt, sigs);
        assert!(minimal_set.is_some());
        let minimal_set = minimal_set.unwrap();
        assert_eq!(minimal_set.proofs.len(), threshold);
        assert!(check_sufficient_block_signatures(&weights, ftt, Some(&minimal_set)).is_ok());
    }

    #[test]
    fn validates_block_signatures() {
        let mut rng = TestRng::new();

        let (keys, weights) = generate_validators(3);
        let mut signatures = create_signatures(&mut rng, &keys, 3);
        assert!(validate_block_signatures(&signatures, &weights).is_ok());

        // Smuggle a bogus proof in.
        let (_, pub_key) = crypto::generate_ed25519_keypair();
        signatures.insert_proof(pub_key.clone(), *signatures.proofs.iter().next().unwrap().1);
        assert!(matches!(
            validate_block_signatures(&signatures, &weights),
            Err(BlockSignatureError::BogusValidators {
                trusted_validator_weights: _,
                block_signatures,
                bogus_validators
            }) if bogus_validators[0] == pub_key && *block_signatures == signatures
        ));
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
    fn block_signatures_sufficiency() {
        const TOTAL_VALIDATORS: usize = 20;
        const TOTAL_VALIDATORS_WEIGHT: usize = TOTAL_VALIDATORS * TEST_VALIDATOR_WEIGHT;
        const INSUFFICIENT_BLOCK_SIGNATURES: usize = 13;
        const JUST_ENOUGH_BLOCK_SIGNATURES: usize = 14;

        let mut rng = TestRng::new();

        // Total validator weights is 20 (1 for each validator).
        let (validators, validator_weights) = generate_validators(TOTAL_VALIDATORS);

        let fault_tolerance_fraction = Ratio::new_raw(1, 3);

        // for 20 validators with 20 total validator weight,
        //   and `fault_tolerance_fraction` = 1/3 (~=  6.666)
        //   and the `quorum fraction` = 2/3         (~= 13.333)
        //
        // we need signatures of weight:
        //   - 13 or less for `InsufficientWeightForFinality`
        //   - 14 for Ok

        let insufficient = create_signatures(&mut rng, &validators, INSUFFICIENT_BLOCK_SIGNATURES);
        let just_enough_weight =
            create_signatures(&mut rng, &validators, JUST_ENOUGH_BLOCK_SIGNATURES);

        let result = check_sufficient_block_signatures(
            &validator_weights,
            fault_tolerance_fraction,
            Some(&insufficient),
        );
        assert!(matches!(
            result,
            Err(BlockSignatureError::InsufficientWeightForFinality {
                trusted_validator_weights: _,
                block_signatures: _,
                signature_weight,
                total_validator_weight,
                fault_tolerance_fraction: _
            }) if *total_validator_weight == TOTAL_VALIDATORS_WEIGHT.into() && **signature_weight.as_ref().unwrap() == INSUFFICIENT_BLOCK_SIGNATURES.into()
        ));

        let result = check_sufficient_block_signatures(
            &validator_weights,
            fault_tolerance_fraction,
            Some(&just_enough_weight),
        );
        assert!(result.is_ok());
    }

    #[test]
    fn block_signatures_sufficiency_with_quorum_formula() {
        const TOTAL_VALIDATORS: usize = 20;
        const TOTAL_VALIDATORS_WEIGHT: usize = TOTAL_VALIDATORS * TEST_VALIDATOR_WEIGHT;
        const INSUFFICIENT_BLOCK_SIGNATURES: usize = 6;
        const JUST_ENOUGH_BLOCK_SIGNATURES: usize = 7;

        let mut rng = TestRng::new();

        // Total validator weights is 20 (1 for each validator).
        let (validators, validator_weights) = generate_validators(TOTAL_VALIDATORS_WEIGHT);

        let fault_tolerance_fraction = Ratio::new_raw(1, 3);

        // `identity` function is transparent, so the calculated quorum fraction will be equal to
        // the `fault_tolerance_fraction`.
        let custom_quorum_formula = std::convert::identity;

        // for 20 validators with 20 total validator weight,
        //   and `fault_tolerance_fraction` = 1/3 (~= 6.666)
        //   and the `quorum fraction` = 1/3         (~= 6.666)
        //
        // we need signatures of weight:
        //   - 6 or less for `InsufficientWeightForFinality`
        //   - 7 for Ok

        let insufficient = create_signatures(&mut rng, &validators, INSUFFICIENT_BLOCK_SIGNATURES);
        let just_enough_weight =
            create_signatures(&mut rng, &validators, JUST_ENOUGH_BLOCK_SIGNATURES);

        let result = check_sufficient_block_signatures_with_quorum_formula(
            &validator_weights,
            fault_tolerance_fraction,
            Some(&insufficient),
            custom_quorum_formula,
        );
        assert!(matches!(
            result,
            Err(BlockSignatureError::InsufficientWeightForFinality {
                trusted_validator_weights: _,
                block_signatures: _,
                signature_weight,
                total_validator_weight,
                fault_tolerance_fraction: _
            }) if *total_validator_weight == TOTAL_VALIDATORS_WEIGHT.into() && **signature_weight.as_ref().unwrap() == INSUFFICIENT_BLOCK_SIGNATURES.into()
        ));

        let result = check_sufficient_block_signatures_with_quorum_formula(
            &validator_weights,
            fault_tolerance_fraction,
            Some(&just_enough_weight),
            custom_quorum_formula,
        );
        assert!(result.is_ok());
    }

    #[test]
    fn block_signatures_sufficiency_with_quorum_formula_without_signatures() {
        const TOTAL_VALIDATORS: usize = 20;
        const TOTAL_VALIDATORS_WEIGHT: usize = TOTAL_VALIDATORS * TEST_VALIDATOR_WEIGHT;
        let (_, validator_weights) = generate_validators(TOTAL_VALIDATORS_WEIGHT);
        let fault_tolerance_fraction = Ratio::new_raw(1, 3);
        let custom_quorum_formula = std::convert::identity;

        let result = check_sufficient_block_signatures_with_quorum_formula(
            &validator_weights,
            fault_tolerance_fraction,
            None,
            custom_quorum_formula,
        );
        assert!(matches!(
            result,
            Err(BlockSignatureError::InsufficientWeightForFinality {
                trusted_validator_weights: _,
                block_signatures,
                signature_weight,
                total_validator_weight,
                fault_tolerance_fraction: _
            }) if *total_validator_weight == TOTAL_VALIDATORS_WEIGHT.into() && signature_weight.is_none() && block_signatures.is_none()
        ));
    }

    #[test]
    fn detects_bogus_validator() {
        const TOTAL_VALIDATORS: usize = 20;
        const JUST_ENOUGH_BLOCK_SIGNATURES: usize = 14;

        let mut rng = TestRng::new();

        let (validators, validator_weights) = generate_validators(TOTAL_VALIDATORS);
        let fault_tolerance_fraction = Ratio::new_raw(1, 3);

        // Generate correct signatures.
        let mut signatures = create_signatures(&mut rng, &validators, JUST_ENOUGH_BLOCK_SIGNATURES);
        let result = check_sufficient_block_signatures(
            &validator_weights,
            fault_tolerance_fraction,
            Some(&signatures),
        );
        assert!(result.is_ok());

        // Smuggle bogus proofs in.
        let (_, pub_key_1) = crypto::generate_ed25519_keypair();
        signatures.insert_proof(
            pub_key_1.clone(),
            *signatures.proofs.iter().next().unwrap().1,
        );
        let (_, pub_key_2) = crypto::generate_ed25519_keypair();
        signatures.insert_proof(
            pub_key_2.clone(),
            *signatures.proofs.iter().next().unwrap().1,
        );
        let result = check_sufficient_block_signatures(
            &validator_weights,
            fault_tolerance_fraction,
            Some(&signatures),
        );
        let error = result.unwrap_err();
        if let BlockSignatureError::BogusValidators {
            trusted_validator_weights: _,
            block_signatures: _,
            bogus_validators,
        } = error
        {
            assert!(bogus_validators.contains(&pub_key_1));
            assert!(bogus_validators.contains(&pub_key_2));
            assert_eq!(bogus_validators.len(), 2);
        } else {
            panic!("unexpected err: {}", error);
        }
    }
}
