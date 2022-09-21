use std::collections::BTreeMap;

use datasize::DataSize;
use itertools::Itertools;

use crate::types::FinalitySignature;
use casper_types::PublicKey;

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(super) enum SignatureState {
    Vacant,
    Signature(FinalitySignature),
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(super) struct SignatureAcquisition {
    inner: BTreeMap<PublicKey, SignatureState>,
}

impl SignatureAcquisition {
    pub(super) fn new(validators: Vec<PublicKey>) -> Self {
        let mut inner = BTreeMap::new();
        validators
            .into_iter()
            .map(|v| inner.insert(v, SignatureState::Vacant));
        SignatureAcquisition { inner }
    }

    pub(super) fn apply_signature(&mut self, finality_signature: FinalitySignature) {
        if self.inner.contains_key(&finality_signature.public_key) {
            self.inner.insert(
                finality_signature.public_key.clone(),
                SignatureState::Signature(finality_signature),
            );
        }
    }

    pub(super) fn needing_signatures(&self) -> Vec<PublicKey> {
        self.inner
            .iter()
            .filter(|(k, v)| **v == SignatureState::Vacant)
            .map(|(k, _)| k.clone())
            .collect_vec()
    }

    pub(super) fn have_signatures(&self) -> Vec<FinalitySignature> {
        self.inner
            .iter()
            .filter_map(|(k, v)| match v {
                SignatureState::Vacant => None,
                SignatureState::Signature(fs) => Some(fs.clone()),
            })
            .collect()
    }
}
