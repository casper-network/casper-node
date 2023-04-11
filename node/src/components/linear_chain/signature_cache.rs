use std::collections::{hash_map::Entry, HashMap};

use casper_types::{EraId, PublicKey};
use datasize::DataSize;

use crate::types::{BlockHash, BlockSignatures};

#[derive(DataSize, Debug)]
pub(super) struct SignatureCache {
    curr_era: EraId,
    signatures: HashMap<BlockHash, BlockSignatures>,
}

impl SignatureCache {
    pub(super) fn new() -> Self {
        SignatureCache {
            curr_era: EraId::from(0),
            signatures: Default::default(),
        }
    }

    pub(super) fn get(&self, hash: &BlockHash) -> Option<BlockSignatures> {
        self.signatures.get(hash).cloned()
    }

    pub(super) fn insert(&mut self, block_signature: BlockSignatures) {
        // We optimistically assume that most of the signatures that arrive in close temporal
        // proximity refer to the same era.
        if self.curr_era < block_signature.era_id {
            self.signatures.clear();
            self.curr_era = block_signature.era_id;
        }
        match self.signatures.entry(block_signature.block_hash) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().proofs.extend(block_signature.proofs);
            }
            Entry::Vacant(entry) => {
                entry.insert(block_signature);
            }
        }
    }

    /// Returns whether finality signature is known already.
    pub(super) fn known_signature(&self, block_hash: &BlockHash, public_key: &PublicKey) -> bool {
        self.signatures
            .get(block_hash)
            .map_or(false, |bs| bs.has_proof(public_key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::FinalitySignature;
    use casper_types::{testing::TestRng, EraId, Signature};

    use std::collections::BTreeMap;

    #[test]
    fn adding_signatures() {
        let mut rng = TestRng::new();
        let block_hash = BlockHash::random(&mut rng);
        let mut cache = SignatureCache::new();

        // Add first signature for the block.
        let mut block_signatures_a = BlockSignatures::new(block_hash, EraId::new(0));
        let sig_a = FinalitySignature::random_for_block(block_hash, 0);
        block_signatures_a.insert_proof(sig_a.public_key.clone(), sig_a.signature);
        cache.insert(block_signatures_a.clone());
        // Verify that the first signature is cached.
        assert!(cache.known_signature(&block_hash, &sig_a.public_key));
        let returned_signatures_a = cache.get(&block_hash).unwrap();
        assert_eq!(block_signatures_a, returned_signatures_a);

        // Adding more signatures for the same block.
        let mut block_signatures_b = BlockSignatures::new(block_hash, EraId::new(0));
        let sig_b = FinalitySignature::random_for_block(block_hash, 0);
        block_signatures_b.insert_proof(sig_b.public_key.clone(), sig_b.signature);
        cache.insert(block_signatures_b.clone());
        // Verify that the second signature is cached.
        assert!(cache.known_signature(&block_hash, &sig_b.public_key));
        let returned_signatures_b = cache.get(&block_hash).unwrap();

        // Cache should extend previously stored signatures with the new ones.
        let expected: BTreeMap<PublicKey, Signature> = block_signatures_a
            .proofs
            .into_iter()
            .chain(block_signatures_b.proofs.into_iter())
            .collect();
        assert_eq!(expected, returned_signatures_b.proofs);
    }

    #[test]
    fn purge_cache() {
        // Cache is purged when it receives a signature for the newer era than the currently cached
        // signatures.
        let mut rng = TestRng::new();
        let block_hash = BlockHash::random(&mut rng);
        let mut cache = SignatureCache::new();

        // Add signature for a block in era-0.
        let mut block_signatures_a = BlockSignatures::new(block_hash, EraId::new(0));
        let sig_a = FinalitySignature::random_for_block(block_hash, 0);
        block_signatures_a.insert_proof(sig_a.public_key.clone(), sig_a.signature);
        cache.insert(block_signatures_a);

        // Add a signature for a block in era-1.
        let mut block_signatures_b = BlockSignatures::new(block_hash, EraId::new(1));
        let sig_b = FinalitySignature::random_for_block(block_hash, 1);
        block_signatures_b.insert_proof(sig_b.public_key.clone(), sig_b.signature);
        cache.insert(block_signatures_b);

        // Verify that era-0 signature is not cached anymore.
        assert!(!cache.known_signature(&block_hash, &sig_a.public_key));

        // Verify that era-1 signature is cached.
        assert!(cache.known_signature(&block_hash, &sig_b.public_key));
    }
}
