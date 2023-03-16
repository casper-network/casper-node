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
