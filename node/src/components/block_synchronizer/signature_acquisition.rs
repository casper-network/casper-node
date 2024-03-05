use std::collections::{btree_map::Entry, BTreeMap};

use datasize::DataSize;

use casper_types::{FinalitySignature, LegacyRequiredFinality, PublicKey};

use super::block_acquisition::Acceptance;
use crate::types::{EraValidatorWeights, SignatureWeight};

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
enum SignatureState {
    Vacant,
    Pending,
    Signature(Box<FinalitySignature>),
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(super) struct SignatureAcquisition {
    inner: BTreeMap<PublicKey, SignatureState>,
    maybe_is_legacy: Option<bool>,
    signature_weight: SignatureWeight,
    legacy_required_finality: LegacyRequiredFinality,
}

impl SignatureAcquisition {
    pub(super) fn new(
        validators: Vec<PublicKey>,
        legacy_required_finality: LegacyRequiredFinality,
    ) -> Self {
        let inner = validators
            .into_iter()
            .map(|validator| (validator, SignatureState::Vacant))
            .collect();
        let maybe_is_legacy = None;
        SignatureAcquisition {
            inner,
            maybe_is_legacy,
            signature_weight: SignatureWeight::Insufficient,
            legacy_required_finality,
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
        let acceptance = match self.inner.entry(finality_signature.public_key().clone()) {
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

    pub(super) fn set_is_legacy(&mut self, is_legacy: bool) {
        self.maybe_is_legacy = Some(is_legacy);
    }

    pub(super) fn is_legacy(&self) -> bool {
        self.maybe_is_legacy.unwrap_or(false)
    }

    pub(super) fn signature_weight(&self) -> SignatureWeight {
        self.signature_weight
    }

    // Determines signature weight sufficiency based on the type of sync (forward or historical) and
    // the protocol version that the block was created with (pre-1.5 or post-1.5)
    // `requires_strict_finality` determines what the caller requires with regards to signature
    // sufficiency:
    //      * false means that the caller considers `Weak` finality as sufficient
    //      * true means that the caller considers `Strict` finality as sufficient
    pub(super) fn has_sufficient_finality(
        &self,
        is_historical: bool,
        requires_strict_finality: bool,
    ) -> bool {
        if is_historical && self.is_legacy() {
            match self.legacy_required_finality {
                LegacyRequiredFinality::Strict => self
                    .signature_weight
                    .is_sufficient(requires_strict_finality),
                LegacyRequiredFinality::Weak => {
                    self.signature_weight == SignatureWeight::Strict
                        || self.signature_weight == SignatureWeight::Weak
                }
                LegacyRequiredFinality::Any => true,
            }
        } else {
            self.signature_weight
                .is_sufficient(requires_strict_finality)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeSet, fmt::Debug, iter};

    use assert_matches::assert_matches;
    use itertools::Itertools;
    use num_rational::Ratio;
    use rand::Rng;

    use casper_types::{
        testing::TestRng, BlockHash, ChainNameDigest, EraId, FinalitySignatureV2, SecretKey, U512,
    };

    use super::*;

    impl SignatureAcquisition {
        pub(super) fn have_no_vacant(&self) -> bool {
            self.inner.iter().all(|(_, v)| *v != SignatureState::Vacant)
        }
    }

    fn keypair(rng: &mut TestRng) -> (PublicKey, SecretKey) {
        let secret = SecretKey::random(rng);
        let public = PublicKey::from(&secret);

        (public, secret)
    }

    /// Asserts that 2 iterators iterate over the same set of items.
    macro_rules! assert_iter_equal {
        ( $left:expr, $right:expr $(,)? ) => {{
            fn to_btreeset<T: Ord + Debug>(
                left: impl IntoIterator<Item = T>,
                right: impl IntoIterator<Item = T>,
            ) -> (BTreeSet<T>, BTreeSet<T>) {
                (left.into_iter().collect(), right.into_iter().collect())
            }

            let (left, right) = to_btreeset($left, $right);
            assert_eq!(left, right);
        }};
    }

    fn test_finality_with_ratio(finality_threshold: Ratio<u64>, first_weight: SignatureWeight) {
        let rng = &mut TestRng::new();
        let validators = iter::repeat_with(|| keypair(rng)).take(4).collect_vec();
        let block_hash = BlockHash::random(rng);
        let block_height = rng.gen();
        let era_id = EraId::new(rng.gen());
        let chain_name_hash = ChainNameDigest::random(rng);
        let weights = EraValidatorWeights::new(
            era_id,
            validators
                .iter()
                .enumerate()
                .map(|(i, (public, _))| (public.clone(), (i + 1).into()))
                .collect(),
            finality_threshold,
        );
        assert_eq!(U512::from(10), weights.get_total_weight());
        let mut signature_acquisition = SignatureAcquisition::new(
            validators.iter().map(|(p, _)| p.clone()).collect(),
            LegacyRequiredFinality::Strict,
        );

        // Signature for the validator #0 weighting 1:
        let (public_0, secret_0) = validators.get(0).unwrap();
        let finality_signature = FinalitySignatureV2::create(
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            secret_0,
        );
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature.into(), &weights),
            Acceptance::NeededIt
        );
        assert_iter_equal!(signature_acquisition.have_signatures(), [public_0]);
        assert_iter_equal!(signature_acquisition.not_vacant(), [public_0]);
        assert!(signature_acquisition.have_no_vacant() == false);
        assert_iter_equal!(
            signature_acquisition.not_pending(),
            validators.iter().map(|(p, _)| p),
        );

        assert_eq!(signature_acquisition.signature_weight(), first_weight);

        // Signature for the validator #2 weighting 3:
        let (public_2, secret_2) = validators.get(2).unwrap();
        let finality_signature = FinalitySignatureV2::create(
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            secret_2,
        );
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature.into(), &weights),
            Acceptance::NeededIt
        );
        assert_iter_equal!(
            signature_acquisition.have_signatures(),
            [public_0, public_2],
        );
        assert_iter_equal!(signature_acquisition.not_vacant(), [public_0, public_2]);
        assert!(signature_acquisition.have_no_vacant() == false);
        assert_iter_equal!(
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
        let finality_signature = FinalitySignatureV2::create(
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            secret_3,
        );
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature.into(), &weights),
            Acceptance::NeededIt
        );
        assert_iter_equal!(
            signature_acquisition.have_signatures(),
            [public_0, public_2, public_3],
        );
        assert_iter_equal!(
            signature_acquisition.not_vacant(),
            [public_0, public_2, public_3],
        );
        assert!(signature_acquisition.have_no_vacant() == false);
        assert_iter_equal!(
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
    fn should_return_insufficient_when_weight_1_and_1_3_is_required() {
        test_finality_with_ratio(Ratio::new(1, 3), SignatureWeight::Insufficient)
    }

    #[test]
    fn should_return_weak_when_weight_1_and_1_10_is_required() {
        test_finality_with_ratio(Ratio::new(1, 10), SignatureWeight::Insufficient)
    }

    #[test]
    fn should_return_weak_when_weight_1_and_1_11_is_required() {
        test_finality_with_ratio(Ratio::new(1, 11), SignatureWeight::Weak)
    }

    #[test]
    fn adding_a_not_already_stored_validator_signature_works() {
        let rng = &mut TestRng::new();
        let validators = iter::repeat_with(|| keypair(rng)).take(4).collect_vec();
        let block_hash = BlockHash::random(rng);
        let block_height = rng.gen();
        let chain_name_hash = ChainNameDigest::random(rng);
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
        let mut signature_acquisition = SignatureAcquisition::new(
            validators.iter().map(|(p, _)| p.clone()).collect(),
            LegacyRequiredFinality::Strict,
        );

        // Signature for an already stored validator:
        let (_public_0, secret_0) = validators.first().unwrap();
        let finality_signature = FinalitySignatureV2::create(
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            secret_0,
        );
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature.into(), &weights),
            Acceptance::NeededIt
        );

        // Signature for an unknown validator:
        let (_public, secret) = keypair(rng);
        let finality_signature =
            FinalitySignatureV2::create(block_hash, block_height, era_id, chain_name_hash, &secret);
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature.into(), &weights),
            Acceptance::NeededIt
        );
    }

    #[test]
    fn signing_twice_does_nothing() {
        let rng = &mut TestRng::new();
        let validators = iter::repeat_with(|| keypair(rng)).take(4).collect_vec();
        let block_hash = BlockHash::random(rng);
        let block_height = rng.gen();
        let chain_name_hash = ChainNameDigest::random(rng);
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
        let mut signature_acquisition = SignatureAcquisition::new(
            validators.iter().map(|(p, _)| p.clone()).collect(),
            LegacyRequiredFinality::Strict,
        );

        let (_public_0, secret_0) = validators.first().unwrap();

        // Signature for an already stored validator:
        let finality_signature = FinalitySignatureV2::create(
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            secret_0,
        );
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature.into(), &weights),
            Acceptance::NeededIt
        );

        // Signing again returns `HadIt`:
        let finality_signature = FinalitySignatureV2::create(
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            secret_0,
        );
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature.into(), &weights),
            Acceptance::HadIt
        );
    }

    #[test]
    fn register_pending_has_the_expected_behavior() {
        let rng = &mut TestRng::new();
        let validators = iter::repeat_with(|| keypair(rng)).take(4).collect_vec();
        let block_hash = BlockHash::random(rng);
        let block_height = rng.gen();
        let era_id = EraId::new(rng.gen());
        let chain_name_hash = ChainNameDigest::random(rng);
        let weights = EraValidatorWeights::new(
            era_id,
            validators
                .iter()
                .enumerate()
                .map(|(i, (public, _))| (public.clone(), (i + 1).into()))
                .collect(),
            Ratio::new(1, 11), // Low finality threshold
        );
        assert_eq!(U512::from(10), weights.get_total_weight());
        let mut signature_acquisition = SignatureAcquisition::new(
            validators.iter().map(|(p, _)| p.clone()).collect(),
            LegacyRequiredFinality::Strict,
        );

        // Set the validator #0 weighting 1 as pending:
        let (public_0, secret_0) = validators.get(0).unwrap();
        signature_acquisition.register_pending(public_0.clone());
        assert_iter_equal!(signature_acquisition.have_signatures(), []);
        assert_iter_equal!(signature_acquisition.not_vacant(), [public_0]);
        assert_iter_equal!(
            signature_acquisition.not_pending(),
            validators.iter().skip(1).map(|(p, _s)| p).collect_vec(),
        );
        assert!(signature_acquisition.have_no_vacant() == false);
        assert_eq!(
            signature_acquisition.signature_weight(),
            SignatureWeight::Insufficient
        );

        // Sign it:
        let finality_signature = FinalitySignatureV2::create(
            block_hash,
            block_height,
            era_id,
            chain_name_hash,
            secret_0,
        );
        assert_matches!(
            signature_acquisition.apply_signature(finality_signature.into(), &weights),
            Acceptance::NeededIt
        );
        assert_iter_equal!(signature_acquisition.have_signatures(), [public_0]);
        assert_iter_equal!(signature_acquisition.not_vacant(), [public_0]);
        assert!(signature_acquisition.have_no_vacant() == false);
        assert_iter_equal!(
            signature_acquisition.not_pending(),
            validators.iter().map(|(p, _)| p),
        );
        assert_eq!(
            signature_acquisition.signature_weight(),
            SignatureWeight::Weak
        );
    }

    #[test]
    fn register_pending_an_unknown_validator_works() {
        let rng = &mut TestRng::new();
        let validators = iter::repeat_with(|| keypair(rng)).take(4).collect_vec();
        let mut signature_acquisition = SignatureAcquisition::new(
            validators.iter().map(|(p, _)| p.clone()).collect(),
            LegacyRequiredFinality::Strict,
        );

        // Set a new validator as pending:
        let (public, _secret) = keypair(rng);
        signature_acquisition.register_pending(public.clone());
        assert_iter_equal!(signature_acquisition.have_signatures(), []);
        assert_iter_equal!(signature_acquisition.not_vacant(), [&public]);
        assert_iter_equal!(
            signature_acquisition.not_pending(),
            validators.iter().map(|(p, _s)| p),
        );
        assert!(signature_acquisition.have_no_vacant() == false);
    }

    #[test]
    fn missing_legacy_flag_means_not_legacy() {
        let signature_weight = SignatureWeight::Insufficient;
        let legacy_required_finality = LegacyRequiredFinality::Any;

        let sa = SignatureAcquisition {
            inner: Default::default(),
            maybe_is_legacy: None,
            signature_weight,
            legacy_required_finality,
        };

        assert!(!sa.is_legacy())
    }

    #[test]
    fn not_historical_and_not_legacy_and_is_insufficient() {
        let signature_weight = SignatureWeight::Insufficient;

        // This parameter should not affect calculation for not historical and not legacy blocks.
        let legacy_required_finality = [
            LegacyRequiredFinality::Any,
            LegacyRequiredFinality::Weak,
            LegacyRequiredFinality::Strict,
        ];

        legacy_required_finality
            .iter()
            .for_each(|legacy_required_finality| {
                let is_legacy = false;
                let sa = SignatureAcquisition {
                    inner: Default::default(),
                    maybe_is_legacy: Some(is_legacy),
                    signature_weight,
                    legacy_required_finality: *legacy_required_finality,
                };

                let is_historical = false;
                let requires_strict_finality = false;
                let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
                assert!(!result);

                let requires_strict_finality = true;
                let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
                assert!(!result);
            })
    }

    #[test]
    fn not_historical_and_not_legacy_and_is_weak() {
        let signature_weight = SignatureWeight::Weak;

        // This parameter should not affect calculation for not historical and not legacy blocks.
        let legacy_required_finality = [
            LegacyRequiredFinality::Any,
            LegacyRequiredFinality::Weak,
            LegacyRequiredFinality::Strict,
        ];

        legacy_required_finality
            .iter()
            .for_each(|legacy_required_finality| {
                let is_legacy = false;
                let sa = SignatureAcquisition {
                    inner: Default::default(),
                    maybe_is_legacy: Some(is_legacy),
                    signature_weight,
                    legacy_required_finality: *legacy_required_finality,
                };

                let is_historical = false;
                let requires_strict_finality = false;
                let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
                assert!(result);

                let requires_strict_finality = true;
                let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
                assert!(!result);
            })
    }

    #[test]
    fn not_historical_and_not_legacy_and_is_strict() {
        let signature_weight = SignatureWeight::Strict;

        // This parameter should not affect calculation for not historical and not legacy blocks.
        let legacy_required_finality = [
            LegacyRequiredFinality::Any,
            LegacyRequiredFinality::Weak,
            LegacyRequiredFinality::Strict,
        ];

        legacy_required_finality
            .iter()
            .for_each(|legacy_required_finality| {
                let is_legacy = false;
                let sa = SignatureAcquisition {
                    inner: Default::default(),
                    maybe_is_legacy: Some(is_legacy),
                    signature_weight,
                    legacy_required_finality: *legacy_required_finality,
                };

                let is_historical = false;
                let requires_strict_finality = false;
                let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
                assert!(result);

                let requires_strict_finality = true;
                let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
                assert!(result);
            })
    }

    #[test]
    fn historical_and_legacy_requires_any_and_is_insufficient() {
        let signature_weight = SignatureWeight::Insufficient;
        let legacy_required_finality = LegacyRequiredFinality::Any;

        let is_legacy = true;
        let sa = SignatureAcquisition {
            inner: Default::default(),
            maybe_is_legacy: Some(is_legacy),
            signature_weight,
            legacy_required_finality,
        };

        let is_historical = true;
        let requires_strict_finality = false;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(result);

        let requires_strict_finality = true;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(result);
    }

    #[test]
    fn historical_and_legacy_requires_any_and_is_weak() {
        let signature_weight = SignatureWeight::Weak;
        let legacy_required_finality = LegacyRequiredFinality::Any;

        let is_legacy = true;
        let sa = SignatureAcquisition {
            inner: Default::default(),
            maybe_is_legacy: Some(is_legacy),
            signature_weight,
            legacy_required_finality,
        };

        let is_historical = true;
        let requires_strict_finality = false;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(result);

        let requires_strict_finality = true;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(result);
    }

    #[test]
    fn historical_and_legacy_requires_any_and_is_strict() {
        let signature_weight = SignatureWeight::Strict;
        let legacy_required_finality = LegacyRequiredFinality::Any;

        let is_legacy = true;
        let sa = SignatureAcquisition {
            inner: Default::default(),
            maybe_is_legacy: Some(is_legacy),
            signature_weight,
            legacy_required_finality,
        };

        let is_historical = true;
        let requires_strict_finality = false;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(result);

        let requires_strict_finality = true;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(result);
    }

    #[test]
    fn historical_and_legacy_requires_weak_and_is_insufficient() {
        let signature_weight = SignatureWeight::Insufficient;
        let legacy_required_finality = LegacyRequiredFinality::Weak;

        let is_legacy = true;
        let sa = SignatureAcquisition {
            inner: Default::default(),
            maybe_is_legacy: Some(is_legacy),
            signature_weight,
            legacy_required_finality,
        };

        let is_historical = true;
        let requires_strict_finality = false;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(!result);

        let requires_strict_finality = true;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(!result);
    }

    #[test]
    fn historical_and_legacy_requires_weak_and_is_weak() {
        let signature_weight = SignatureWeight::Weak;
        let legacy_required_finality = LegacyRequiredFinality::Weak;

        let is_legacy = true;
        let sa = SignatureAcquisition {
            inner: Default::default(),
            maybe_is_legacy: Some(is_legacy),
            signature_weight,
            legacy_required_finality,
        };

        let is_historical = true;
        let requires_strict_finality = false;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(result);

        let requires_strict_finality = true;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(result);
    }

    #[test]
    fn historical_and_legacy_requires_weak_and_is_strict() {
        let signature_weight = SignatureWeight::Strict;
        let legacy_required_finality = LegacyRequiredFinality::Weak;

        let is_legacy = true;
        let sa = SignatureAcquisition {
            inner: Default::default(),
            maybe_is_legacy: Some(is_legacy),
            signature_weight,
            legacy_required_finality,
        };

        let is_historical = true;
        let requires_strict_finality = false;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(result);

        let requires_strict_finality = true;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(result);
    }

    #[test]
    fn historical_and_legacy_requires_strict_and_is_insufficient() {
        let signature_weight = SignatureWeight::Insufficient;
        let legacy_required_finality = LegacyRequiredFinality::Strict;

        let is_legacy = true;
        let sa = SignatureAcquisition {
            inner: Default::default(),
            maybe_is_legacy: Some(is_legacy),
            signature_weight,
            legacy_required_finality,
        };

        let is_historical = true;
        let requires_strict_finality = false;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(!result);

        let requires_strict_finality = true;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(!result);
    }

    #[test]
    fn historical_and_legacy_requires_strict_and_is_weak() {
        let signature_weight = SignatureWeight::Weak;
        let legacy_required_finality = LegacyRequiredFinality::Strict;

        let is_legacy = true;
        let sa = SignatureAcquisition {
            inner: Default::default(),
            maybe_is_legacy: Some(is_legacy),
            signature_weight,
            legacy_required_finality,
        };

        let is_historical = true;
        let requires_strict_finality = false;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(result);

        let requires_strict_finality = true;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(!result);
    }

    #[test]
    fn historical_and_legacy_requires_strict_and_is_strict() {
        let signature_weight = SignatureWeight::Strict;
        let legacy_required_finality = LegacyRequiredFinality::Strict;

        let is_legacy = true;
        let sa = SignatureAcquisition {
            inner: Default::default(),
            maybe_is_legacy: Some(is_legacy),
            signature_weight,
            legacy_required_finality,
        };

        let is_historical = true;
        let requires_strict_finality = false;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(result);

        let requires_strict_finality = true;
        let result = sa.has_sufficient_finality(is_historical, requires_strict_finality);
        assert!(result);
    }
}
