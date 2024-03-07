use std::collections::BTreeMap;

use num::rational::Ratio;
use thiserror::Error;

use casper_types::{BlockSignatures, PublicKey, U512};

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
            for public_key in block_signatures.signers() {
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
                    bogus_validators,
                });
            }

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
        bogus_validators: Vec<PublicKey>,
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

#[cfg(test)]
mod tests {
    use rand::Rng;

    use casper_types::{
        crypto, testing::TestRng, BlockHash, BlockSignaturesV2, ChainNameDigest, EraId,
        FinalitySignature, SecretKey,
    };

    use super::*;

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
    ) -> BlockSignaturesV2 {
        let era_id = EraId::new(rng.gen_range(10..100));

        let block_hash = BlockHash::random(rng);
        let block_height = rng.gen();
        let chain_name_hash = ChainNameDigest::random(rng);

        let mut sigs = BlockSignaturesV2::new(block_hash, block_height, era_id, chain_name_hash);

        for (pub_key, secret_key) in validators.iter().take(n_sigs) {
            let sig = crypto::sign(block_hash, secret_key, pub_key);
            sigs.insert_signature(pub_key.clone(), sig);
        }

        sigs
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
            Some(&BlockSignatures::from(insufficient)),
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
            Some(&BlockSignatures::from(just_enough_weight)),
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
            Some(&BlockSignatures::from(insufficient)),
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
            Some(&BlockSignatures::from(just_enough_weight)),
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
            Some(&BlockSignatures::from(signatures.clone())),
        );
        assert!(result.is_ok());

        // Smuggle bogus proofs in.
        let block_hash = *signatures.block_hash();
        let block_height = signatures.block_height();
        let era_id = signatures.era_id();
        let chain_name_hash = signatures.chain_name_hash();
        let finality_sig_1 = FinalitySignature::random_for_block(
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            &mut rng,
        );
        signatures.insert_signature(
            finality_sig_1.public_key().clone(),
            *finality_sig_1.signature(),
        );
        let finality_sig_2 = FinalitySignature::random_for_block(
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            &mut rng,
        );
        signatures.insert_signature(
            finality_sig_2.public_key().clone(),
            *finality_sig_2.signature(),
        );
        let result = check_sufficient_block_signatures(
            &validator_weights,
            fault_tolerance_fraction,
            Some(&BlockSignatures::from(signatures)),
        );
        let error = result.unwrap_err();
        if let BlockSignatureError::BogusValidators {
            trusted_validator_weights: _,
            block_signatures: _,
            bogus_validators,
        } = error
        {
            assert!(bogus_validators.contains(finality_sig_1.public_key()));
            assert!(bogus_validators.contains(finality_sig_2.public_key()));
            assert_eq!(bogus_validators.len(), 2);
        } else {
            panic!("unexpected err: {}", error);
        }
    }
}
