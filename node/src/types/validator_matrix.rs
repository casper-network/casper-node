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
    #[data_size(skip)]
    finality_threshold_fraction: Ratio<u64>,
    secret_signing_key: Arc<SecretKey>,
    public_signing_key: PublicKey,
}

impl ValidatorMatrix {
    pub(crate) fn new(
        finality_threshold_fraction: Ratio<u64>,
        secret_signing_key: Arc<SecretKey>,
        public_signing_key: PublicKey,
    ) -> Self {
        let inner = Arc::new(RwLock::new(BTreeMap::new()));
        ValidatorMatrix {
            inner,
            finality_threshold_fraction,
            secret_signing_key,
            public_signing_key,
        }
    }

    /// Creates a new validator matrix with just a single validator.
    #[cfg(test)]
    pub(crate) fn new_with_validator(
        secret_signing_key: Arc<SecretKey>,
        public_signing_key: PublicKey,
    ) -> Self {
        let finality_threshold_fraction = Ratio::new(1, 3);
        let era_id = EraId::new(0);
        let weights = EraValidatorWeights::new(
            era_id,
            iter::once((public_signing_key.clone(), 100.into())).collect(),
            finality_threshold_fraction,
        );
        ValidatorMatrix {
            inner: Arc::new(RwLock::new(iter::once((era_id, weights)).collect())),
            finality_threshold_fraction,
            public_signing_key,
            secret_signing_key,
        }
    }

    pub(crate) fn public_signing_key(&self) -> &PublicKey {
        &self.public_signing_key
    }

    pub(crate) fn register_era_validator_weights(
        &mut self,
        validators: EraValidatorWeights,
    ) -> bool {
        let era_id = validators.era_id;
        self.inner
            .write()
            .unwrap()
            .insert(era_id, validators)
            .is_none()
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

    pub(crate) fn validator_weights(&self, era_id: EraId) -> Option<EraValidatorWeights> {
        self.read_inner().get(&era_id).cloned()
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
        self.read_inner()
            .get(&era_id)
            .map(|validator_weights| validator_weights.is_validator(public_key))
    }

    pub(crate) fn is_validator_in_any_of_latest_n_eras(
        &self,
        n: usize,
        public_key: &PublicKey,
    ) -> bool {
        self.read_inner()
            .values()
            .rev()
            .take(n)
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
