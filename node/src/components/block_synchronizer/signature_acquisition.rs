use std::collections::{btree_map::Entry, BTreeMap};

use datasize::DataSize;

use casper_types::PublicKey;

use super::block_acquisition::Acceptance;
use crate::types::{EraValidatorWeights, FinalitySignature, SignatureWeight};

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
enum SignatureState {
    Vacant,
    Pending,
    Signature(Box<FinalitySignature>),
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(super) struct SignatureAcquisition {
    inner: BTreeMap<PublicKey, SignatureState>,
    maybe_is_checkable: Option<bool>,
    signature_weight: SignatureWeight,
}

impl SignatureAcquisition {
    pub(super) fn new(validators: Vec<PublicKey>) -> Self {
        let inner = validators
            .into_iter()
            .map(|validator| (validator, SignatureState::Vacant))
            .collect();
        let maybe_is_checkable = None;
        SignatureAcquisition {
            inner,
            maybe_is_checkable,
            signature_weight: SignatureWeight::Insufficient,
        }
    }

    pub(super) fn register_pending(&mut self, public_key: PublicKey) {
        match self.inner.entry(public_key) {
            Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(SignatureState::Pending);
            }
            Entry::Occupied(mut occupied_entry) => {
                if *occupied_entry.get() == SignatureState::Vacant {
                    occupied_entry.insert(SignatureState::Pending);
                }
            }
        }
    }

    pub(super) fn apply_signature(
        &mut self,
        finality_signature: FinalitySignature,
        validator_weights: &EraValidatorWeights,
    ) -> Acceptance {
        let acceptance = match self.inner.entry(finality_signature.public_key.clone()) {
            Entry::Vacant(vacant_entry) => {
                vacant_entry.insert(SignatureState::Signature(Box::new(finality_signature)));
                Acceptance::NeededIt
            }
            Entry::Occupied(mut occupied_entry) => match *occupied_entry.get() {
                SignatureState::Vacant | SignatureState::Pending => {
                    occupied_entry.insert(SignatureState::Signature(Box::new(finality_signature)));
                    Acceptance::NeededIt
                }
                SignatureState::Signature(_) => Acceptance::HadIt,
            },
        };
        if self.signature_weight != SignatureWeight::Strict {
            self.signature_weight = validator_weights.signature_weight(self.have_signatures());
        }
        acceptance
    }

    pub(super) fn have_signatures(&self) -> impl Iterator<Item = &PublicKey> {
        self.inner.iter().filter_map(|(k, v)| match v {
            SignatureState::Vacant | SignatureState::Pending => None,
            SignatureState::Signature(_finality_signature) => Some(k),
        })
    }

    pub(super) fn not_vacant(&self) -> impl Iterator<Item = &PublicKey> {
        self.inner.iter().filter_map(|(k, v)| match v {
            SignatureState::Vacant => None,
            SignatureState::Pending | SignatureState::Signature(_) => Some(k),
        })
    }

    pub(super) fn not_pending(&self) -> impl Iterator<Item = &PublicKey> {
        self.inner.iter().filter_map(|(k, v)| match v {
            SignatureState::Pending => None,
            SignatureState::Vacant | SignatureState::Signature(_) => Some(k),
        })
    }

    pub(super) fn have_no_vacant(&self) -> bool {
        self.inner.iter().all(|(_, v)| *v != SignatureState::Vacant)
    }

    pub(super) fn set_is_checkable(&mut self, is_checkable: bool) {
        self.maybe_is_checkable = Some(is_checkable)
    }

    pub(super) fn is_checkable(&self) -> bool {
        self.maybe_is_checkable.unwrap_or(false)
    }

    pub(super) fn signature_weight(&self) -> SignatureWeight {
        self.signature_weight
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fmt::Debug};

    use super::*;
    use crate::types::BlockHash;
    use assert_matches::assert_matches;
    use casper_types::{testing::TestRng, EraId, SecretKey, U512};
    use itertools::Itertools;
    use num_rational::Ratio;
    use rand::Rng;

    fn validators(rng: &mut TestRng, n: usize) -> Vec<(PublicKey, SecretKey)> {
        (0..n)
            .map(|_| {
                let secret = SecretKey::random(rng);
                let public = PublicKey::from(&secret);

                (public, secret)
            })
            .collect()
    }

    fn assert_equal<T: Ord + Debug>(
        left: impl IntoIterator<Item = T>,
        right: impl IntoIterator<Item = T>,
    ) {
        let left: BTreeSet<_> = left.into_iter().collect();
        let right: BTreeSet<_> = right.into_iter().collect();

        itertools::assert_equal(left, right);
    }

    #[test]
    fn apply_signature_has_the_expected_behavior_highway_finality() {
        let rng = &mut TestRng::new();
        let validators = validators(rng, 4);
        let block_hash = BlockHash::random(rng);
        let era_id = EraId::new(rng.gen());
        let weights = EraValidatorWeights::new(
            era_id,
            validators
                .iter()
                .enumerate()
                .map(|(i, (public, _))| (public.clone(), (i + 1).into()))
                .collect(),
            Ratio::new(1, 3), // Highway finality
        );
        assert_eq!(U512::from(10), weights.get_total_weight());
        let mut signature_acquisition =
            SignatureAcquisition::new(validators.iter().map(|(p, _)| p.clone()).collect());

        // Signature for the validator #0 weighting 1:
        let (public_0, secret_0) = validators.get(0).unwrap();
        let finality_signature =
            FinalitySignature::create(block_hash, era_id, secret_0, public_0.clone());
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature, &weights),
            Acceptance::NeededIt
        );
        assert_equal(signature_acquisition.have_signatures(), [public_0]);
        assert_equal(signature_acquisition.not_vacant(), [public_0]);
        assert!(signature_acquisition.have_no_vacant() == false);
        assert_equal(
            signature_acquisition.not_pending(),
            validators.iter().map(|(p, _)| p),
        );
        assert_eq!(
            signature_acquisition.signature_weight(),
            SignatureWeight::Insufficient
        );

        // Signature for the validator #2 weighting 3:
        let (public_2, secret_2) = validators.get(2).unwrap();
        let finality_signature =
            FinalitySignature::create(block_hash, era_id, secret_2, public_2.clone());
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature, &weights),
            Acceptance::NeededIt
        );
        assert_equal(
            signature_acquisition.have_signatures(),
            [public_0, public_2],
        );
        assert_equal(signature_acquisition.not_vacant(), [public_0, public_2]);
        assert!(signature_acquisition.have_no_vacant() == false);
        assert_equal(
            signature_acquisition.not_pending(),
            validators.iter().map(|(p, _)| p),
        );
        // The total signed weight is 4/10, which is higher than 1/3:
        assert_eq!(
            signature_acquisition.signature_weight(),
            SignatureWeight::Weak
        );

        // Signature for the validator #3 weighting 4:
        let (public_3, secret_3) = validators.get(3).unwrap();
        let finality_signature =
            FinalitySignature::create(block_hash, era_id, secret_3, public_3.clone());
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature, &weights),
            Acceptance::NeededIt
        );
        assert_equal(
            signature_acquisition.have_signatures(),
            [public_0, public_2, public_3],
        );
        assert_equal(
            signature_acquisition.not_vacant(),
            [public_0, public_2, public_3],
        );
        assert!(signature_acquisition.have_no_vacant() == false);
        assert_equal(
            signature_acquisition.not_pending(),
            validators.iter().map(|(p, _)| p),
        );
        // The total signed weight is 8/10, which is higher than 2/3:
        assert_eq!(
            signature_acquisition.signature_weight(),
            SignatureWeight::Strict
        );
    }

    #[test]
    fn apply_signature_has_the_expected_behavior_low_finality() {
        let rng = &mut TestRng::new();
        let validators = validators(rng, 4);
        let block_hash = BlockHash::random(rng);
        let era_id = EraId::new(rng.gen());
        let weights = EraValidatorWeights::new(
            era_id,
            validators
                .iter()
                .enumerate()
                .map(|(i, (public, _))| (public.clone(), (i + 1).into()))
                .collect(),
            Ratio::new(1, 10), // Low finality threshold
        );
        assert_eq!(U512::from(10), weights.get_total_weight());
        let mut signature_acquisition =
            SignatureAcquisition::new(validators.iter().map(|(p, _)| p.clone()).collect());

        // Signature for the validator #0 weighting 1:
        let (public_0, secret_0) = validators.get(0).unwrap();
        let finality_signature =
            FinalitySignature::create(block_hash, era_id, secret_0, public_0.clone());
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature, &weights),
            Acceptance::NeededIt
        );
        assert_equal(signature_acquisition.have_signatures(), [public_0]);
        assert_equal(signature_acquisition.not_vacant(), [public_0]);
        assert!(signature_acquisition.have_no_vacant() == false);
        assert_equal(
            signature_acquisition.not_pending(),
            validators.iter().map(|(p, _)| p),
        );
        // We get the 1/10 needed:
        assert_eq!(
            signature_acquisition.signature_weight(),
            SignatureWeight::Weak
        );

        // Signature for the validator #2 weighting 3:
        let (public_2, secret_2) = validators.get(2).unwrap();
        let finality_signature =
            FinalitySignature::create(block_hash, era_id, secret_2, public_2.clone());
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature, &weights),
            Acceptance::NeededIt
        );
        assert_equal(
            signature_acquisition.have_signatures(),
            [public_0, public_2],
        );
        assert_equal(signature_acquisition.not_vacant(), [public_0, public_2]);
        assert!(signature_acquisition.have_no_vacant() == false);
        assert_equal(
            signature_acquisition.not_pending(),
            validators.iter().map(|(p, _)| p),
        );
        // The total signed weight is 4/10, which is higher than 1/3:
        assert_eq!(
            signature_acquisition.signature_weight(),
            SignatureWeight::Weak
        );

        // Signature for the validator #3 weighting 4:
        let (public_3, secret_3) = validators.get(3).unwrap();
        let finality_signature =
            FinalitySignature::create(block_hash, era_id, secret_3, public_3.clone());
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature, &weights),
            Acceptance::NeededIt
        );
        assert_equal(
            signature_acquisition.have_signatures(),
            [public_0, public_2, public_3],
        );
        assert_equal(
            signature_acquisition.not_vacant(),
            [public_0, public_2, public_3],
        );
        assert!(signature_acquisition.have_no_vacant() == false);
        assert_equal(
            signature_acquisition.not_pending(),
            validators.iter().map(|(p, _)| p),
        );
        // The total signed weight is 8/10, which is higher than 2/3:
        assert_eq!(
            signature_acquisition.signature_weight(),
            SignatureWeight::Strict
        );
    }

    #[test]
    fn register_pending_has_the_expected_behavior() {
        let rng = &mut TestRng::new();
        let validators = validators(rng, 4);
        let era_id = EraId::new(rng.gen());
        let block_hash = BlockHash::random(rng);
        let weights = EraValidatorWeights::new(
            era_id,
            validators
                .iter()
                .enumerate()
                .map(|(i, (public, _))| (public.clone(), (i + 1).into()))
                .collect(),
            Ratio::new(1, 10), // Low finality threshold
        );
        assert_eq!(U512::from(10), weights.get_total_weight());
        let mut signature_acquisition =
            SignatureAcquisition::new(validators.iter().map(|(p, _)| p.clone()).collect());

        // Set the validator #0 weighting 1 as pending:
        let (public_0, secret_0) = validators.get(0).unwrap();
        signature_acquisition.register_pending(public_0.clone());
        assert_equal(signature_acquisition.have_signatures(), []);
        assert_equal(signature_acquisition.not_vacant(), [public_0]);
        assert_equal(
            signature_acquisition.not_pending(),
            validators.iter().skip(1).map(|(p, _s)| p).collect_vec(),
        );
        assert!(signature_acquisition.have_no_vacant() == false);
        assert_eq!(
            signature_acquisition.signature_weight(),
            SignatureWeight::Insufficient
        );

        // Sign it:
        let finality_signature =
            FinalitySignature::create(block_hash, era_id, secret_0, public_0.clone());
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature, &weights),
            Acceptance::NeededIt
        );
        assert_equal(signature_acquisition.have_signatures(), [public_0]);
        assert_equal(signature_acquisition.not_vacant(), [public_0]);
        assert!(signature_acquisition.have_no_vacant() == false);
        assert_equal(
            signature_acquisition.not_pending(),
            validators.iter().map(|(p, _)| p),
        );
        assert_eq!(
            signature_acquisition.signature_weight(),
            SignatureWeight::Weak
        );
    }

    /*
    #[test]
    fn invalid_signature_does_not_apply() {
        let rng = &mut TestRng::new();
        let validators = validators(rng, 4);
        let block_hash = BlockHash::random(rng);
        let era_id = EraId::new(rng.gen());
        let weights = EraValidatorWeights::new(
            era_id,
            validators
                .iter()
                .enumerate()
                .map(|(i, (public, _))| (public.clone(), (i + 1).into()))
                .collect(),
            Ratio::new(1, 10), // Low finality threshold
        );
        assert_eq!(U512::from(10), weights.get_total_weight());
        let mut signature_acquisition =
            SignatureAcquisition::new(validators.iter().map(|(p, _)| p.clone()).collect());

        let (public_0, _secret_0) = validators.get(0).unwrap();
        let (_public_1, secret_1) = validators.get(1).unwrap();
        let finality_signature =
            FinalitySignature::create(block_hash, era_id, secret_1, public_0.clone());

        // This should panic because of the secret/public mismatch:
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature, &weights),
            Acceptance::NeededIt
        );
        //panic!(
        //    "{:?}",
        //    signature_acquisition.have_signatures().collect_vec()
        //);
        //assert_equal(signature_acquisition.have_signatures(), []);

        assert_eq!(
            signature_acquisition.signature_weight(),
            SignatureWeight::Insufficient
        );
    }
    */
}
