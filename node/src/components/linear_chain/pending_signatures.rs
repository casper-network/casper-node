use datasize::DataSize;
use itertools::Itertools;
use std::collections::HashMap;
use tracing::warn;

use super::signature::Signature;
use crate::types::BlockHash;
use casper_types::PublicKey;

/// The maximum number of finality signatures from a single validator we keep in memory while
/// waiting for their block.
const MAX_PENDING_FINALITY_SIGNATURES_PER_VALIDATOR: usize = 1000;

/// Finality signatures to be inserted in a block once it is available.
/// Keyed by public key of the creator to limit the maximum amount of pending signatures.
#[derive(DataSize, Debug, Default)]
pub(super) struct PendingSignatures {
    pending_finality_signatures: HashMap<PublicKey, HashMap<BlockHash, Signature>>,
}

impl PendingSignatures {
    pub(super) fn new() -> Self {
        PendingSignatures {
            pending_finality_signatures: HashMap::new(),
        }
    }

    // Checks if we have already enqueued that finality signature.
    pub(super) fn has_finality_signature(
        &self,
        creator: &PublicKey,
        block_hash: &BlockHash,
    ) -> bool {
        self.pending_finality_signatures
            .get(creator)
            .map_or(false, |sigs| sigs.contains_key(block_hash))
    }

    /// Returns signatures for `block_hash` that are still pending.
    pub(super) fn collect_pending(&mut self, block_hash: &BlockHash) -> Vec<Signature> {
        let pending_sigs = self
            .pending_finality_signatures
            .values_mut()
            .filter_map(|sigs| sigs.remove(&block_hash))
            .collect_vec();
        self.remove_empty_entries();
        pending_sigs
    }

    pub(super) fn add(&mut self, signature: Signature) {
        let public_key = signature.public_key();
        let block_hash = signature.block_hash();
        let sigs = self
            .pending_finality_signatures
            .entry(public_key)
            .or_default();
        // Limit the memory we use for storing unknown signatures from each validator.
        if sigs.len() >= MAX_PENDING_FINALITY_SIGNATURES_PER_VALIDATOR {
            warn!(
                %block_hash, %public_key,
                "received too many finality signatures for unknown blocks"
            );
            return;
        }
        // Add the pending signature.
        let _ = sigs.insert(block_hash, signature);
    }

    pub(super) fn remove(
        &mut self,
        public_key: &PublicKey,
        block_hash: &BlockHash,
    ) -> Option<Signature> {
        let validator_sigs = self.pending_finality_signatures.get_mut(public_key)?;
        let sig = validator_sigs.remove(&block_hash);
        self.remove_empty_entries();
        sig
    }

    /// Removes all entries for which there are no finality signatures.
    fn remove_empty_entries(&mut self) {
        self.pending_finality_signatures
            .retain(|_, sigs| !sigs.is_empty());
    }
}
