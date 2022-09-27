use std::{
    collections::{btree_map::Entry, BTreeMap, HashSet},
    sync::{Arc, RwLock},
};

use datasize::DataSize;
use itertools::Itertools;
use num_rational::Ratio;
use serde::Serialize;

use crate::types::FinalitySignature;
use casper_types::{EraId, PublicKey, U512};

#[derive(Eq, PartialEq, Debug, Copy, Clone)]
pub(crate) enum SignatureWeight {
    /// Too few signatures to make any guarantees about the block's finality.
    Insufficient,
    /// At least one honest validator has signed the block.
    Weak,
    /// There can be no blocks on other forks that also have this many signatures.
    Sufficient,
}

#[derive(Clone, DataSize, Debug, Serialize, Default)]
pub(crate) struct ValidatorMatrix {
    inner: Arc<RwLock<BTreeMap<EraId, EraValidatorWeights>>>,
    #[data_size(skip)]
    finality_threshold_fraction: Ratio<u64>,
}

impl ValidatorMatrix {
    pub(crate) fn new(finality_threshold_fraction: Ratio<u64>) -> Self {
        let inner = Arc::new(RwLock::new(BTreeMap::new()));
        ValidatorMatrix {
            inner,
            finality_threshold_fraction,
        }
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
        if self.inner.read().unwrap().contains_key(&era_id) == false {
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

    pub(crate) fn upsert(&mut self, validators: BTreeMap<EraId, EraValidatorWeights>) {
        let mut writer = self.inner.write().unwrap();
        for (era_id, ev) in validators {
            writer.insert(era_id, ev);
        }
    }

    pub(crate) fn remove_era(&mut self, era_id: EraId) {
        self.inner.write().unwrap().remove(&era_id);
    }

    pub(crate) fn remove_eras(&mut self, earliest_era_to_keep: EraId) {
        let mut writer = self.inner.write().unwrap();
        *writer = writer.split_off(&earliest_era_to_keep);
    }

    pub(crate) fn validator_weights(&self, era_id: EraId) -> Option<EraValidatorWeights> {
        self.inner.read().unwrap().get(&era_id).cloned()
    }

    pub(crate) fn validator_public_keys(&self, era_id: EraId) -> Option<Vec<PublicKey>> {
        Some(
            self.inner
                .read()
                .unwrap()
                .get(&era_id)?
                .validator_public_keys()
                .cloned()
                .collect(),
        )
    }

    pub(crate) fn missing_validators<'a>(
        &self,
        era_id: EraId,
        validator_keys: impl Iterator<Item = &'a PublicKey>,
    ) -> Option<Vec<PublicKey>> {
        Some(
            self.inner
                .read()
                .unwrap()
                .get(&era_id)?
                .missing_validators(validator_keys)
                .cloned()
                .collect(),
        )
    }

    pub(crate) fn get_weight(&self, era_id: EraId, public_key: &PublicKey) -> U512 {
        match self.inner.read().unwrap().get(&era_id) {
            None => U512::zero(),
            Some(ev) => ev.get_weight(public_key),
        }
    }

    pub(crate) fn get_total_weight(&self, era_id: EraId) -> Option<U512> {
        Some(self.inner.read().unwrap().get(&era_id)?.get_total_weight())
    }

    pub(crate) fn has_sufficient_weight<'a>(
        &self,
        era_id: EraId,
        validator_keys: impl Iterator<Item = &'a PublicKey>,
    ) -> Option<SignatureWeight> {
        Some(
            self.inner
                .read()
                .unwrap()
                .get(&era_id)?
                .has_sufficient_weight(validator_keys),
        )
    }

    pub(crate) fn bogus_validators<'a>(
        &self,
        era_id: EraId,
        validator_keys: impl Iterator<Item = &'a PublicKey>,
    ) -> Option<Vec<PublicKey>> {
        Some(
            self.inner
                .read()
                .unwrap()
                .get(&era_id)?
                .bogus_validators(validator_keys),
        )
    }

    pub(crate) fn fault_tolerance_threshold(&self) -> Ratio<u64> {
        self.finality_threshold_fraction
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

    pub(crate) fn weights(&self) -> &BTreeMap<PublicKey, U512> {
        &self.validator_weights
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
