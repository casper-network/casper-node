use std::collections::BTreeMap;

use datasize::DataSize;
use itertools::Itertools;
use num_rational::Ratio;

use crate::types::FinalitySignature;
use casper_types::{EraId, PublicKey, U512};

pub(crate) enum SignatureWeight {
    Insufficient,
    Sufficient,
    Strict,
}

#[derive(DataSize, Debug)]
pub(crate) struct ValidatorMatrix {
    inner: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
    #[data_size(skip)]
    fault_tolerance_fraction: Ratio<u64>,
}

impl ValidatorMatrix {
    pub(crate) fn new(fault_tolerance_fraction: Ratio<u64>) -> Self {
        ValidatorMatrix {
            inner: BTreeMap::new(),
            fault_tolerance_fraction,
        }
    }

    pub(crate) fn register_era(&mut self, era_id: EraId, validators: BTreeMap<PublicKey, U512>) {
        self.inner.insert(era_id, validators);
    }

    pub(crate) fn remove_era(&mut self, era_id: EraId) {
        self.inner.remove(&era_id);
    }

    pub(crate) fn remove_eras(&mut self, earliest_era_to_keep: EraId) {
        self.inner = self.inner.split_off(&earliest_era_to_keep);
    }

    pub(crate) fn validator_public_keys(&self, era_id: EraId) -> Option<Vec<PublicKey>> {
        self.inner
            .get(&era_id)
            .map(|validators| validators.keys().cloned().collect())
    }

    pub(crate) fn missing_signatures(
        &self,
        era_id: EraId,
        signatures: &[FinalitySignature],
    ) -> Result<Vec<PublicKey>, ()> {
        let signed = signatures
            .iter()
            .map(|fs| fs.public_key.clone())
            .collect_vec();
        let mut ret = vec![];
        match self.inner.get(&era_id) {
            Some(validators) => {
                for (k, v) in validators {
                    if signed.contains(k) == false {
                        ret.push(k.clone());
                    }
                }
            }
            None => return Err(()),
        }
        Ok(ret)
    }

    pub(crate) fn get_weight(&self, era_id: EraId, public_key: &PublicKey) -> U512 {
        match self.inner.get(&era_id) {
            None => U512::zero(),
            Some(inner) => match inner.get(public_key) {
                None => U512::zero(),
                Some(w) => *w,
            },
        }
    }

    pub(crate) fn get_total_weight(&self, era_id: EraId) -> Option<U512> {
        Some(self.inner.get(&era_id)?.values().copied().sum())
    }

    pub(crate) fn have_sufficient_weight(
        &self,
        era_id: EraId,
        signatures: Vec<FinalitySignature>,
    ) -> SignatureWeight {
        if signatures.is_empty() {
            return SignatureWeight::Insufficient;
        }
        match self.get_total_weight(era_id) {
            None => {
                return SignatureWeight::Insufficient;
            }
            Some(total_era_weight) => {
                let fault_tolerance_fraction = self.fault_tolerance_fraction;
                let strict = Ratio::new(1, 2) * (Ratio::from_integer(1) + fault_tolerance_fraction);
                let signature_weight: U512 = signatures
                    .iter()
                    .map(|i| self.get_weight(era_id, &i.public_key))
                    .sum();
                if signature_weight * U512::from(*strict.denom())
                    >= total_era_weight * U512::from(*strict.numer())
                {
                    return SignatureWeight::Strict;
                }
                if signature_weight * U512::from(*fault_tolerance_fraction.denom())
                    >= total_era_weight * U512::from(*fault_tolerance_fraction.numer())
                {
                    return SignatureWeight::Sufficient;
                }
            }
        }
        SignatureWeight::Insufficient
    }
}
