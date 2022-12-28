#[cfg(test)]
use std::iter;
use std::{
    collections::{BTreeMap, HashSet},
    fmt::{self, Debug, Formatter},
    sync::{Arc, RwLock, RwLockReadGuard},
};

use datasize::DataSize;
use itertools::Itertools;
use num_rational::Ratio;
use serde::Serialize;

use casper_types::{EraId, PublicKey, SecretKey, U512};

use super::{BlockHeader, FinalitySignature};

const MAX_VALIDATOR_MATRIX_ENTRIES: usize = 6;

#[derive(Eq, PartialEq, Debug, Copy, Clone)]
pub(crate) enum SignatureWeight {
    /// Too few signatures to make any guarantees about the block's finality.
    Insufficient,
    /// At least one honest validator has signed the block.
    Weak,
    /// There can be no blocks on other forks that also have this many signatures.
    Sufficient,
}

#[derive(Clone, DataSize)]
pub(crate) struct ValidatorMatrix {
    inner: Arc<RwLock<BTreeMap<EraId, EraValidatorWeights>>>,
    chainspec_validators: Option<Arc<BTreeMap<PublicKey, U512>>>,
    chainspec_activation_era: EraId,
    #[data_size(skip)]
    finality_threshold_fraction: Ratio<u64>,
    secret_signing_key: Arc<SecretKey>,
    public_signing_key: PublicKey,
    auction_delay: u64,
}

impl ValidatorMatrix {
    pub(crate) fn new(
        finality_threshold_fraction: Ratio<u64>,
        chainspec_validators: Option<BTreeMap<PublicKey, U512>>,
        chainspec_activation_era: EraId,
        secret_signing_key: Arc<SecretKey>,
        public_signing_key: PublicKey,
        auction_delay: u64,
    ) -> Self {
        let inner = Arc::new(RwLock::new(BTreeMap::new()));
        ValidatorMatrix {
            inner,
            finality_threshold_fraction,
            chainspec_validators: chainspec_validators.map(Arc::new),
            chainspec_activation_era,
            secret_signing_key,
            public_signing_key,
            auction_delay,
        }
    }

    /// Creates a new validator matrix with just a single validator.
    #[cfg(test)]
    pub(crate) fn new_with_validator(secret_signing_key: Arc<SecretKey>) -> Self {
        let public_signing_key = PublicKey::from(&*secret_signing_key);
        let finality_threshold_fraction = Ratio::new(1, 3);
        let era_id = EraId::new(0);
        let weights = EraValidatorWeights::new(
            era_id,
            iter::once((public_signing_key.clone(), 100.into())).collect(),
            finality_threshold_fraction,
        );
        ValidatorMatrix {
            inner: Arc::new(RwLock::new(iter::once((era_id, weights)).collect())),
            chainspec_validators: None,
            chainspec_activation_era: EraId::from(0),
            finality_threshold_fraction,
            public_signing_key,
            secret_signing_key,
            auction_delay: 2,
        }
    }

    pub(crate) fn register_era_validator_weights(
        &mut self,
        validators: EraValidatorWeights,
    ) -> bool {
        let era_id = validators.era_id;
        let mut guard = self
            .inner
            .write()
            .expect("poisoned lock on validator matrix");
        let was_present = guard.insert(era_id, validators).is_some();
        if guard.len() > MAX_VALIDATOR_MATRIX_ENTRIES {
            // Safe to unwrap because we check above that we have sufficient entries.
            let median_key = guard
                .keys()
                .nth(MAX_VALIDATOR_MATRIX_ENTRIES / 2)
                .copied()
                .unwrap();
            guard.remove(&median_key);
            return median_key != era_id;
        }
        !was_present
    }

    pub(crate) fn register_validator_weights(
        &mut self,
        era_id: EraId,
        validator_weights: BTreeMap<PublicKey, U512>,
    ) {
        if self.read_inner().contains_key(&era_id) == false {
            self.register_era_validator_weights(EraValidatorWeights::new(
                era_id,
                validator_weights,
                self.finality_threshold_fraction,
            ));
        }
    }

    pub(crate) fn register_eras(
        &mut self,
        era_weights: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
    ) {
        for (era_id, weights) in era_weights {
            self.register_validator_weights(era_id, weights);
        }
    }

    pub(crate) fn has_era(&self, era_id: &EraId) -> bool {
        self.read_inner().contains_key(era_id)
    }

    pub(crate) fn validator_weights(&self, era_id: EraId) -> Option<EraValidatorWeights> {
        if let (true, Some(chainspec_validators)) = (
            era_id == self.chainspec_activation_era,
            self.chainspec_validators.as_ref(),
        ) {
            Some(EraValidatorWeights::new(
                era_id,
                (**chainspec_validators).clone(),
                self.finality_threshold_fraction,
            ))
        } else {
            self.read_inner().get(&era_id).cloned()
        }
    }

    pub(crate) fn fault_tolerance_threshold(&self) -> Ratio<u64> {
        self.finality_threshold_fraction
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.read_inner().is_empty()
    }

    /// Returns whether `pub_key` is the ID of a validator in this era, or `None` if the validator
    /// information for that era is missing.
    pub(crate) fn is_validator_in_era(
        &self,
        era_id: EraId,
        public_key: &PublicKey,
    ) -> Option<bool> {
        if let (true, Some(chainspec_validators)) = (
            era_id == self.chainspec_activation_era,
            self.chainspec_validators.as_ref(),
        ) {
            Some(chainspec_validators.contains_key(public_key))
        } else {
            self.read_inner()
                .get(&era_id)
                .map(|validator_weights| validator_weights.is_validator(public_key))
        }
    }

    /// Determine if the active validator is in a current or upcoming set of active validators.
    #[inline]
    pub(crate) fn is_active_or_upcoming_validator(&self, public_key: &PublicKey) -> bool {
        // This function is potentially expensive and could be memoized, with the cache being
        // invalidated when the max value of the `BTreeMap` changes.
        self.read_inner()
            .values()
            .rev()
            .take(self.auction_delay as usize + 1)
            .any(|validator_weights| validator_weights.is_validator(public_key))
    }

    pub(crate) fn create_finality_signature(
        &self,
        block_header: &BlockHeader,
    ) -> Option<FinalitySignature> {
        if self
            .is_validator_in_era(block_header.era_id(), &self.public_signing_key)
            .unwrap_or(false)
        {
            return Some(FinalitySignature::create(
                block_header.block_hash(),
                block_header.era_id(),
                &self.secret_signing_key,
                self.public_signing_key.clone(),
            ));
        }
        None
    }

    fn read_inner(&self) -> RwLockReadGuard<BTreeMap<EraId, EraValidatorWeights>> {
        self.inner.read().unwrap()
    }
}

impl Debug for ValidatorMatrix {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ValidatorMatrix")
            .field("weights", &*self.read_inner())
            .field(
                "finality_threshold_fraction",
                &self.finality_threshold_fraction,
            )
            .finish()
    }
}

#[derive(DataSize, Debug, Serialize, Default, Clone)]
pub(crate) struct EraValidatorWeights {
    era_id: EraId,
    validator_weights: BTreeMap<PublicKey, U512>,
    #[data_size(skip)]
    finality_threshold_fraction: Ratio<u64>,
}

impl EraValidatorWeights {
    pub(crate) fn new(
        era_id: EraId,
        validator_weights: BTreeMap<PublicKey, U512>,
        finality_threshold_fraction: Ratio<u64>,
    ) -> Self {
        EraValidatorWeights {
            era_id,
            validator_weights,
            finality_threshold_fraction,
        }
    }

    pub(crate) fn era_id(&self) -> EraId {
        self.era_id
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.validator_weights.is_empty()
    }

    pub(crate) fn get_total_weight(&self) -> U512 {
        self.validator_weights.values().copied().sum()
    }

    pub(crate) fn validator_public_keys(&self) -> impl Iterator<Item = &PublicKey> {
        self.validator_weights.keys()
    }

    pub(crate) fn missing_validators<'a>(
        &self,
        validator_keys: impl Iterator<Item = &'a PublicKey>,
    ) -> impl Iterator<Item = &PublicKey> {
        let provided_keys: HashSet<_> = validator_keys.cloned().collect();
        self.validator_weights
            .keys()
            .filter(move |&validator| !provided_keys.contains(validator))
    }

    pub(crate) fn bogus_validators<'a>(
        &self,
        validator_keys: impl Iterator<Item = &'a PublicKey>,
    ) -> Vec<PublicKey> {
        validator_keys
            .filter(move |validator_key| !self.validator_weights.keys().contains(validator_key))
            .cloned()
            .collect()
    }

    pub(crate) fn get_weight(&self, public_key: &PublicKey) -> U512 {
        match self.validator_weights.get(public_key) {
            None => U512::zero(),
            Some(w) => *w,
        }
    }

    pub(crate) fn is_validator(&self, public_key: &PublicKey) -> bool {
        self.validator_weights.contains_key(public_key)
    }

    pub(crate) fn has_sufficient_weight<'a>(
        &self,
        validator_keys: impl Iterator<Item = &'a PublicKey>,
    ) -> SignatureWeight {
        // sufficient is ~33.4%, strict is ~66.7% by default in highway
        // in some cases, we may already have strict weight or better before even starting.
        // this is optimal, but in the cases where we do not we are willing to start work
        // on acquiring block data on a block for which we have at least sufficient weight.
        // nevertheless, we will try to attain strict weight before fully accepting such
        // a block.
        let finality_threshold_fraction = self.finality_threshold_fraction;
        let strict = Ratio::new(1, 2) * (Ratio::from_integer(1) + finality_threshold_fraction);
        let total_era_weight = self.get_total_weight();
        let signature_weight: U512 = validator_keys
            .map(|validator_key| self.get_weight(validator_key))
            .sum();
        if signature_weight * U512::from(*strict.denom())
            >= total_era_weight * U512::from(*strict.numer())
        {
            return SignatureWeight::Sufficient;
        }
        if signature_weight * U512::from(*finality_threshold_fraction.denom())
            >= total_era_weight * U512::from(*finality_threshold_fraction.numer())
        {
            return SignatureWeight::Weak;
        }
        SignatureWeight::Insufficient
    }
}

#[cfg(test)]
mod tests {
    use std::iter;

    use casper_types::EraId;
    use num_rational::Ratio;

    use crate::{
        components::consensus::tests::utils::{ALICE_PUBLIC_KEY, ALICE_SECRET_KEY},
        types::validator_matrix::MAX_VALIDATOR_MATRIX_ENTRIES,
    };

    use super::{EraValidatorWeights, ValidatorMatrix};

    fn empty_era_validator_weights(era_id: EraId) -> EraValidatorWeights {
        EraValidatorWeights::new(
            era_id,
            iter::once((ALICE_PUBLIC_KEY.clone(), 100.into())).collect(),
            Ratio::new(1, 3),
        )
    }

    #[test]
    fn register_validator_weights_pruning() {
        // Create a validator matrix and saturate it with entries.
        let mut validator_matrix = ValidatorMatrix::new_with_validator(ALICE_SECRET_KEY.clone());
        let mut era_validator_weights = vec![validator_matrix.validator_weights(0.into()).unwrap()];
        era_validator_weights.extend(
            (1..MAX_VALIDATOR_MATRIX_ENTRIES as u64)
                .into_iter()
                .map(EraId::from)
                .map(empty_era_validator_weights),
        );
        for evw in era_validator_weights
            .iter()
            .take(MAX_VALIDATOR_MATRIX_ENTRIES)
            .skip(1)
            .cloned()
        {
            assert!(validator_matrix.register_era_validator_weights(evw));
        }

        // Now that we have 6 entries in the validator matrix, try adding more.
        // We should have an entry for era 3 (we have eras 0 through 5
        // inclusive).
        assert!(validator_matrix.has_era(&(MAX_VALIDATOR_MATRIX_ENTRIES as u64 / 2).into()));
        // Add era 7.
        era_validator_weights.push(empty_era_validator_weights(
            (MAX_VALIDATOR_MATRIX_ENTRIES as u64 + 1).into(),
        ));
        // Now the entry for era 3 should be dropped and we should be left with
        // the 3 lowest eras [0, 1, 2] and 3 highest eras [4, 5, 6].
        assert!(validator_matrix
            .register_era_validator_weights(era_validator_weights.last().cloned().unwrap()));
        assert!(!validator_matrix.has_era(&(MAX_VALIDATOR_MATRIX_ENTRIES as u64 / 2).into()));
        assert_eq!(
            validator_matrix.read_inner().len(),
            MAX_VALIDATOR_MATRIX_ENTRIES
        );

        // Adding existing eras shouldn't change the state.
        let old_state: Vec<EraId> = validator_matrix.read_inner().keys().copied().collect();
        assert!(!validator_matrix
            .register_era_validator_weights(era_validator_weights.last().cloned().unwrap()));
        let new_state: Vec<EraId> = validator_matrix.read_inner().keys().copied().collect();
        assert_eq!(old_state, new_state);

        // Adding an entry greater than the 3 lowest ones but less than the 3
        // highest ones should not change state.
        assert!(!validator_matrix.register_era_validator_weights(era_validator_weights[3].clone()));
        let new_state: Vec<EraId> = validator_matrix.read_inner().keys().copied().collect();
        assert_eq!(old_state, new_state);
    }
}
