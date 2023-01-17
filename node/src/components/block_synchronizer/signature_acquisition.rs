use std::collections::{btree_map::Entry, BTreeMap};

use datasize::DataSize;

use crate::types::FinalitySignature;
use casper_types::PublicKey;

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

    /// Returns `true` if new signature was registered.
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

    pub(crate) fn set_is_checkable(&mut self, is_checkable: bool) {
        self.maybe_is_checkable = Some(is_checkable)
    }

    pub(crate) fn requires_strict_finality(&self, is_recent_block: bool) -> bool {
        if is_recent_block {
            return true;
        }

        self.maybe_is_checkable.unwrap_or(false)
    }
}
