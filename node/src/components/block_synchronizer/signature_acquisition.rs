use std::collections::BTreeMap;

use datasize::DataSize;

use crate::types::FinalitySignature;
use casper_types::PublicKey;

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
enum SignatureState {
    Vacant,
    Signature(Box<FinalitySignature>),
}

#[derive(Clone, PartialEq, Eq, DataSize, Debug)]
pub(super) struct SignatureAcquisition {
    inner: BTreeMap<PublicKey, SignatureState>,
}

impl SignatureAcquisition {
    pub(super) fn new(validators: Vec<PublicKey>) -> Self {
        let inner = validators
            .into_iter()
            .map(|validator| (validator, SignatureState::Vacant))
            .collect();
        SignatureAcquisition { inner }
    }

    // Returns `true` if new signature was registered.
    pub(super) fn apply_signature(&mut self, finality_signature: FinalitySignature) -> bool {
        self.inner
            .insert(
                finality_signature.public_key.clone(),
                SignatureState::Signature(Box::new(finality_signature)),
            )
            .is_none()
    }

    pub(super) fn have_signatures(&self) -> impl Iterator<Item = &PublicKey> {
        self.inner.iter().filter_map(|(k, v)| match v {
            SignatureState::Vacant => None,
            SignatureState::Signature(_finality_signature) => Some(k),
        })
    }
}
