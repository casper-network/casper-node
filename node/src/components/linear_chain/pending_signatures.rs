use datasize::DataSize;
use itertools::Itertools;
use std::collections::{hash_map::Entry, HashMap};
use tracing::warn;

use super::signature::Signature;
use crate::types::BlockHash;
use casper_types::PublicKey;

/// The maximum number of finality signatures from a single validator we keep in memory while
/// waiting for their block.
const MAX_PENDING_FINALITY_SIGNATURES_PER_VALIDATOR: usize = 1000;

#[derive(DataSize, Debug)]
enum VerificationStatus {
    Unknown(Signature),
    Bonded(Signature),
}

impl VerificationStatus {
    fn is_bonded(&self) -> bool {
        matches!(self, &VerificationStatus::Bonded(_))
    }

    fn into_inner(self) -> Signature {
        match self {
            Self::Unknown(sig) | Self::Bonded(sig) => sig,
        }
    }

    fn value(&self) -> &Signature {
        match self {
            Self::Unknown(sig) | Self::Bonded(sig) => sig,
        }
    }
}

/// Finality signatures to be inserted in a block once it is available.
/// Keyed by public key of the creator to limit the maximum amount of pending signatures.
#[derive(DataSize, Debug, Default)]
pub(super) struct PendingSignatures {
    pending_finality_signatures: HashMap<PublicKey, HashMap<BlockHash, VerificationStatus>>,
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
            .filter_map(|sigs| {
                // Collect the signature only if it's been verified already.
                match sigs.get(block_hash) {
                    Some(sig_status) if sig_status.is_bonded() => sigs.remove(block_hash),
                    _ => None,
                }
            })
            .map(|status| status.into_inner())
            .collect_vec();
        self.remove_empty_entries();
        pending_sigs
    }

    /// Adds finality signature to the pending collection.
    /// Returns `true` if it was added.
    pub(super) fn add(&mut self, signature: Signature) -> bool {
        let public_key = signature.public_key();
        let block_hash = signature.block_hash();
        let sigs = self
            .pending_finality_signatures
            .entry(public_key.clone())
            .or_default();
        // Limit the memory we use for storing unknown signatures from each validator.
        if sigs.len() >= MAX_PENDING_FINALITY_SIGNATURES_PER_VALIDATOR {
            warn!(
                %block_hash, %public_key,
                "received too many finality signatures for unknown blocks"
            );
            return false;
        }

        // Add the pending signature.
        let value = VerificationStatus::Unknown(signature);
        sigs.insert(block_hash, value);
        true
    }

    pub(super) fn remove(
        &mut self,
        public_key: &PublicKey,
        block_hash: &BlockHash,
    ) -> Option<Signature> {
        let validator_sigs = self.pending_finality_signatures.get_mut(public_key)?;
        let sig = validator_sigs.remove(block_hash);
        self.remove_empty_entries();
        sig.map(|status| status.into_inner())
    }

    /// Marks pending finality signature as created by a bonded validator.
    /// Returns whether there was a signature from the `public_key` for `block_hash` in the
    /// collection.
    pub(super) fn mark_bonded(&mut self, public_key: PublicKey, block_hash: BlockHash) -> bool {
        self.mark_bonded_helper(public_key, block_hash).is_some()
    }

    fn mark_bonded_helper(&mut self, public_key: PublicKey, block_hash: BlockHash) -> Option<()> {
        match self.pending_finality_signatures.entry(public_key.clone()) {
            Entry::Occupied(mut validator_sigs) => {
                let sig = validator_sigs.get_mut().get(&block_hash)?;
                let bonded_status = VerificationStatus::Bonded(sig.value().clone());
                validator_sigs.get_mut().insert(block_hash, bonded_status);
                Some(())
            }
            Entry::Vacant(_) => {
                warn!(?public_key, ?block_hash, "no signature from validator");
                None
            }
        }
    }

    /// Removes all entries for which there are no finality signatures.
    fn remove_empty_entries(&mut self) {
        self.pending_finality_signatures
            .retain(|_, sigs| !sigs.is_empty());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FinalitySignature;
    use casper_types::{crypto::generate_ed25519_keypair, testing::TestRng, EraId};

    use std::collections::BTreeMap;

    #[test]
    fn membership_test() {
        let mut rng = TestRng::new();
        let mut pending_sigs = PendingSignatures::new();
        let block_hash = BlockHash::random(&mut rng);
        let block_hash_other = BlockHash::random(&mut rng);
        let sig_a = FinalitySignature::random_for_block(block_hash, 0);
        let sig_b = FinalitySignature::random_for_block(block_hash_other, 0);
        let public_key = sig_a.public_key.clone();
        let public_key_other = sig_b.public_key;
        assert!(pending_sigs.add(Signature::External(Box::new(sig_a))));
        assert!(pending_sigs.has_finality_signature(&public_key, &block_hash));
        assert!(!pending_sigs.has_finality_signature(&public_key_other, &block_hash));
        assert!(!pending_sigs.has_finality_signature(&public_key, &block_hash_other));
    }

    #[test]
    fn collect_pending() {
        let mut rng = TestRng::new();
        let mut pending_sigs = PendingSignatures::new();
        let block_hash = BlockHash::random(&mut rng);
        let block_hash_other = BlockHash::random(&mut rng);
        let sig_a1 = FinalitySignature::random_for_block(block_hash, 0);
        let sig_a2 = FinalitySignature::random_for_block(block_hash, 0);
        let sig_b = FinalitySignature::random_for_block(block_hash_other, 0);
        assert!(pending_sigs.add(Signature::External(Box::new(sig_a1.clone()))));
        assert!(pending_sigs.mark_bonded(sig_a1.public_key.clone(), block_hash));
        assert!(pending_sigs.add(Signature::External(Box::new(sig_a2.clone()))));
        assert!(pending_sigs.mark_bonded(sig_a2.public_key.clone(), block_hash));
        assert!(pending_sigs.add(Signature::External(Box::new(sig_b))));
        let collected_sigs: BTreeMap<PublicKey, FinalitySignature> = pending_sigs
            .collect_pending(&block_hash)
            .into_iter()
            .map(|sig| (sig.public_key(), *sig.take()))
            .collect();
        let expected_sigs = vec![sig_a1.clone(), sig_a2.clone()]
            .into_iter()
            .map(|sig| (sig.public_key.clone(), sig))
            .collect();
        assert_eq!(collected_sigs, expected_sigs);
        assert!(
            !pending_sigs.has_finality_signature(&sig_a1.public_key, &sig_a1.block_hash),
            "collecting should remove the signature"
        );
        assert!(
            !pending_sigs.has_finality_signature(&sig_a2.public_key, &sig_a2.block_hash),
            "collecting should remove the signature"
        );
    }

    #[test]
    fn remove_signature() {
        let mut rng = TestRng::new();
        let mut pending_sigs = PendingSignatures::new();
        let block_hash = BlockHash::random(&mut rng);
        let sig = FinalitySignature::random_for_block(block_hash, 0);
        assert!(pending_sigs.add(Signature::External(Box::new(sig.clone()))));
        let removed_sig = pending_sigs.remove(&sig.public_key, &sig.block_hash);
        assert!(removed_sig.is_some());
        assert!(!pending_sigs.has_finality_signature(&sig.public_key, &sig.block_hash));
        assert!(pending_sigs
            .remove(&sig.public_key, &sig.block_hash)
            .is_none());
    }

    #[test]
    fn max_limit_respected() {
        let mut rng = TestRng::new();
        let mut pending_sigs = PendingSignatures::new();
        let (sec_key, pub_key) = generate_ed25519_keypair();
        let era_id = EraId::new(0);
        for _ in 0..MAX_PENDING_FINALITY_SIGNATURES_PER_VALIDATOR {
            let block_hash = BlockHash::random(&mut rng);
            let sig = FinalitySignature::new(block_hash, era_id, &sec_key, pub_key.clone());
            assert!(pending_sigs.add(Signature::External(Box::new(sig))));
        }
        let block_hash = BlockHash::random(&mut rng);
        let sig = FinalitySignature::new(block_hash, era_id, &sec_key, pub_key);
        assert!(!pending_sigs.add(Signature::External(Box::new(sig))));
    }

    #[test]
    fn mark_bonded_does_not_remove() {
        // This is a unit test added after a bug was spotted during the code review
        // where marking a (public_key, block_hash) pair as bonded twice would remove the entry from
        // the collection.
        let mut rng = TestRng::new();
        let mut pending_sigs = PendingSignatures::new();
        let block_hash = BlockHash::random(&mut rng);
        let sig_a = FinalitySignature::random_for_block(block_hash, 0);
        let public_key = sig_a.public_key.clone();
        assert!(pending_sigs.add(Signature::External(Box::new(sig_a))));
        assert!(pending_sigs.has_finality_signature(&public_key, &block_hash));
        assert!(pending_sigs.mark_bonded(public_key.clone(), block_hash));
        assert!(pending_sigs.has_finality_signature(&public_key, &block_hash));
        // Verify that marking the same entry as bonded the second time does not remove it.
        assert!(pending_sigs.mark_bonded(public_key.clone(), block_hash));
        assert!(pending_sigs.has_finality_signature(&public_key, &block_hash));
    }
}
