use std::collections::HashMap;

use datasize::DataSize;
use itertools::Itertools;
use tracing::{debug, warn};

use crate::{
    crypto::hash::Digest,
    types::{Block, BlockHash, BlockSignatures, DeployHash, FinalitySignature},
};
use casper_types::{ExecutionResult, ProtocolVersion};

use super::{
    pending_signatures::PendingSignatures, signature::Signature, signature_cache::SignatureCache,
};
#[derive(DataSize, Debug)]
pub(crate) struct LinearChain {
    /// The most recently added block.
    latest_block: Option<Block>,
    /// Finality signatures to be inserted in a block once it is available.
    pending_finality_signatures: PendingSignatures,
    signature_cache: SignatureCache,
    /// Current protocol version of the network.
    protocol_version: ProtocolVersion,
    auction_delay: u64,
    unbonding_delay: u64,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Outcome {
    // Store block signatures to storage.
    StoreBlockSignatures(BlockSignatures),
    // Store execution results to storage.
    StoreExecutionResults(BlockHash, HashMap<DeployHash, ExecutionResult>),
    // Store block.
    StoreBlock(Box<Block>),
    // Read finality signatures for the block from storage.
    LoadSignatures(Box<FinalitySignature>),
    // Gossip finality signature to peers.
    Gossip(Box<FinalitySignature>),
    // Create a reactor announcement about new (valid) finality signatures.
    AnnounceSignature(Box<FinalitySignature>),
    // Create a reactor announcement about new (valid) block.
    AnnounceBlock(Box<Block>),
    // Check if creator of `new_fs` is known trusted validator.
    // Carries additional context necessary to create the corresponding event.
    VerifyIfBonded {
        new_fs: Box<FinalitySignature>,
        known_fs: Option<Box<BlockSignatures>>,
        protocol_version: ProtocolVersion,
        latest_state_root_hash: Option<Digest>,
    },
}

pub(super) type Outcomes = Vec<Outcome>;

impl LinearChain {
    pub(crate) fn new(
        protocol_version: ProtocolVersion,
        auction_delay: u64,
        unbonding_delay: u64,
    ) -> Self {
        LinearChain {
            latest_block: None,
            pending_finality_signatures: PendingSignatures::new(),
            signature_cache: SignatureCache::new(),
            protocol_version,
            auction_delay,
            unbonding_delay,
        }
    }

    /// Returns whether we have already enqueued that finality signature.
    fn is_pending(&self, fs: &FinalitySignature) -> bool {
        let creator = fs.public_key;
        let block_hash = fs.block_hash;
        self.pending_finality_signatures
            .has_finality_signature(&creator, &block_hash)
    }

    /// Returns whether we have already seen and stored the finality signature.
    fn is_new(&self, fs: &FinalitySignature) -> bool {
        let FinalitySignature {
            block_hash,
            public_key,
            ..
        } = fs;
        !self.signature_cache.known_signature(block_hash, public_key)
    }

    // New linear chain block received. Collect any pending finality signatures that
    // were waiting for that block.
    fn new_block(&mut self, block: &Block) -> Vec<Signature> {
        let signatures = self.collect_pending_finality_signatures(block.hash());
        if signatures.is_empty() {
            return vec![];
        }
        let mut block_signatures = BlockSignatures::new(*block.hash(), block.header().era_id());
        for sig in signatures.iter() {
            block_signatures.insert_proof(sig.public_key(), sig.signature());
        }
        // Cache the signatures as we expect more finality signatures for the new block to
        // arrive soon.
        self.cache_signatures(block_signatures);
        signatures
    }

    /// Tries to add the finality signature to the collection of pending finality signatures.
    /// Returns true if added successfully, otherwise false.
    fn add_pending_finality_signature(&mut self, fs: FinalitySignature, gossiped: bool) -> bool {
        let FinalitySignature {
            block_hash,
            public_key,
            era_id,
            ..
        } = fs;
        if let Some(latest_block) = self.latest_block.as_ref() {
            // If it's a switch block it has already forgotten its own era's validators,
            // unbonded some old validators, and determined new ones. In that case, we
            // should add 1 to last_block_era.
            let current_era = latest_block.header().era_id()
                + if latest_block.header().is_switch_block() {
                    1
                } else {
                    0
                };
            let lowest_acceptable_era_id =
                (current_era + self.auction_delay).saturating_sub(self.unbonding_delay);
            let highest_acceptable_era_id = current_era + self.auction_delay;
            if era_id < lowest_acceptable_era_id || era_id > highest_acceptable_era_id {
                warn!(
                    era_id=%era_id.value(),
                    %public_key,
                    %block_hash,
                    "received finality signature for not bonded era."
                );
                return false;
            }
        }
        if self.is_pending(&fs) {
            debug!(block_hash=%fs.block_hash, public_key=%fs.public_key,
                "finality signature already pending");
            return false;
        }
        if !self.is_new(&fs) {
            debug!(block_hash=%fs.block_hash, public_key=%fs.public_key,
                "finality signature is already known");
            return false;
        }
        if let Err(err) = fs.verify() {
            warn!(%block_hash, %public_key, %err, "received invalid finality signature");
            return false;
        }
        debug!(%block_hash, %public_key, "received new finality signature");
        let signature = if gossiped {
            Signature::External(Box::new(fs))
        } else {
            Signature::Local(Box::new(fs))
        };
        self.pending_finality_signatures.add(signature)
    }

    /// Removes finality signature from the pending collection.
    fn remove_from_pending_fs(&mut self, fs: &FinalitySignature) -> Option<Signature> {
        let FinalitySignature {
            block_hash,
            era_id: _era_id,
            signature: _signature,
            public_key,
        } = fs;
        debug!(%block_hash, %public_key, "removing finality signature from pending collection");
        self.pending_finality_signatures
            .remove(&public_key, &block_hash)
    }

    /// Caches the signature.
    fn cache_signatures(&mut self, mut signatures: BlockSignatures) {
        // Merge already-known signatures and the new ones.
        self.get_signatures(&signatures.block_hash)
            .iter()
            .for_each(|bs| {
                for (pk, sig) in bs.proofs.iter() {
                    signatures.insert_proof(*pk, *sig);
                }
            });
        self.signature_cache.insert(signatures);
    }

    /// Returns cached finality signatures that we have already validated and stored.
    fn get_signatures(&self, block_hash: &BlockHash) -> Option<BlockSignatures> {
        self.signature_cache.get(block_hash)
    }

    fn current_protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    fn set_latest_block(&mut self, block: Block) {
        self.latest_block = Some(block);
    }

    fn latest_block(&self) -> &Option<Block> {
        &self.latest_block
    }

    /// Returns finality signatures for `block_hash`.
    fn collect_pending_finality_signatures(&mut self, block_hash: &BlockHash) -> Vec<Signature> {
        self.pending_finality_signatures
            .collect_pending(block_hash)
            .into_iter()
            .filter(|sig| {
                let FinalitySignature {
                    block_hash,
                    public_key,
                    ..
                } = sig.to_inner();
                !self.signature_cache.known_signature(block_hash, public_key)
            })
            .collect_vec()
    }

    pub(super) fn handle_new_block(
        &mut self,
        block: Box<Block>,
        execution_results: HashMap<DeployHash, ExecutionResult>,
    ) -> Outcomes {
        let mut outcomes = vec![];
        let signatures = self.new_block(&*block);
        if !signatures.is_empty() {
            let mut block_signatures = BlockSignatures::new(*block.hash(), block.header().era_id());
            for sig in signatures.iter() {
                block_signatures.insert_proof(sig.public_key(), sig.signature());
            }
            outcomes.push(Outcome::StoreBlockSignatures(block_signatures));
            for signature in signatures {
                if signature.is_local() {
                    outcomes.push(Outcome::Gossip(Box::new(signature.to_inner().clone())));
                }
                outcomes.push(Outcome::AnnounceSignature(signature.take()));
            }
        };
        let block_hash = *block.hash();
        outcomes.push(Outcome::StoreBlock(block));
        outcomes.push(Outcome::StoreExecutionResults(
            block_hash,
            execution_results,
        ));
        outcomes
    }

    pub(super) fn handle_put_block(&mut self, block: Box<Block>) -> Outcomes {
        self.set_latest_block(*block.clone());
        vec![Outcome::AnnounceBlock(block)]
    }

    pub(super) fn handle_finality_signature(
        &mut self,
        fs: Box<FinalitySignature>,
        gossiped: bool,
    ) -> Outcomes {
        let FinalitySignature { block_hash, .. } = *fs;
        if !self.add_pending_finality_signature(*fs.clone(), gossiped) {
            // If we did not add the signature it means it's either incorrect or we already
            // know it.
            return vec![];
        }
        match self.get_signatures(&block_hash) {
            // Not found in the cache, look in the storage.
            None => vec![Outcome::LoadSignatures(fs)],
            Some(signatures) => self.handle_cached_signatures(Some(Box::new(signatures)), fs),
        }
    }

    pub(super) fn handle_cached_signatures(
        &mut self,
        signatures: Option<Box<BlockSignatures>>,
        fs: Box<FinalitySignature>,
    ) -> Outcomes {
        if let Some(known_signatures) = &signatures {
            // If the newly-received finality signature does not match the era of previously
            // validated signatures reject it as they can't both be
            // correct – block was created in a specific era so the IDs have to match.
            if known_signatures.era_id != fs.era_id {
                warn!(public_key = %fs.public_key,
                    expected = %known_signatures.era_id,
                    got = %fs.era_id,
                    "finality signature with invalid era id.");
                self.remove_from_pending_fs(&*fs);
                // TODO: Disconnect from the sender.
                return vec![];
            }
            if known_signatures.has_proof(&fs.public_key) {
                self.remove_from_pending_fs(&fs);
                return vec![];
            }
            // Populate cache so that next incoming signatures don't trigger read from the
            // storage. If `known_signatures` are already from cache then this will be a
            // noop.
            self.cache_signatures(*known_signatures.clone());
        }
        // Check if the validator is bonded in the era in which the block was created.
        // TODO: Use protocol version that is valid for the block's height.
        let protocol_version = self.current_protocol_version();
        let latest_state_root_hash = self
            .latest_block()
            .as_ref()
            .map(|block| *block.header().state_root_hash());
        vec![Outcome::VerifyIfBonded {
            new_fs: fs,
            known_fs: signatures,
            protocol_version,
            latest_state_root_hash,
        }]
    }

    pub(super) fn handle_is_bonded(
        &mut self,
        maybe_known_signatures: Option<Box<BlockSignatures>>,
        new_fs: Box<FinalitySignature>,
        is_bonded: bool,
    ) -> Outcomes {
        if !is_bonded {
            // Creator of the finality signature (`new_fs`) is not known to be a trusted
            // validator. Neither in the current era nor in the eras for which
            // we have already run auctions for.
            self.remove_from_pending_fs(&new_fs);
            let FinalitySignature {
                public_key,
                block_hash,
                ..
            } = *new_fs;
            warn!(
                validator = %public_key,
                %block_hash,
                "Received a signature from a validator that is not bonded."
            );
            // TODO: Disconnect from the sender.
            return vec![];
        }

        self.pending_finality_signatures
            .mark_bonded(new_fs.public_key, new_fs.block_hash);

        match maybe_known_signatures {
            None => {
                // Unknown block but validator is bonded.
                // We should finalize the same block eventually. Either in this or in the
                // next eras. New signature is already cached for later.
                vec![]
            }
            Some(mut known_signatures) => {
                // New finality signature from a bonded validator.
                known_signatures.insert_proof(new_fs.public_key, new_fs.signature);
                // Cache the results in case we receive the same finality signature before we
                // manage to store it in the database.
                self.cache_signatures(*known_signatures.clone());
                debug!(hash = %known_signatures.block_hash, "storing finality signatures");
                // Announce new finality signatures for other components to pick up.
                let mut outcomes = vec![Outcome::AnnounceSignature(new_fs.clone())];
                if let Some(signature) = self.remove_from_pending_fs(&*new_fs) {
                    // This shouldn't return `None` as we added the `fs` to the pending collection
                    // when we received it. If it _is_ `None` then a concurrent
                    // flow must have already removed it. If it's a signature
                    // created by this node, gossip it.
                    if signature.is_local() {
                        outcomes.push(Outcome::Gossip(new_fs.clone()));
                    }
                };
                outcomes.push(Outcome::StoreBlockSignatures(*known_signatures));
                outcomes
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{crypto::generate_ed25519_keypair, logging, testing::TestRng};
    use casper_types::EraId;

    use super::*;

    #[test]
    fn new_block_no_sigs() {
        let mut rng = TestRng::new();
        let protocol_version = ProtocolVersion::V1_0_0;
        let mut lc = LinearChain::new(protocol_version, 1u64, 1u64);
        let block = Block::random(&mut rng);
        let execution_results = HashMap::new();
        let new_block_outcomes =
            lc.handle_new_block(Box::new(block.clone()), execution_results.clone());
        let block_hash = *block.hash();
        match &*new_block_outcomes {
            [Outcome::StoreBlock(outcome_block), Outcome::StoreExecutionResults(outcome_block_hash, outcome_execution_results)] =>
            {
                assert_eq!(&**outcome_block, &block);
                assert_eq!(outcome_block_hash, &block_hash);
                assert_eq!(outcome_execution_results, &execution_results);
            }
            others => panic!("unexpected outcome: {:?}", others),
        }
        let block_stored_outcomes = lc.handle_put_block(Box::new(block.clone()));
        match &*block_stored_outcomes {
            [Outcome::AnnounceBlock(announced_block)] => {
                assert_eq!(&**announced_block, &block);
            }
            others => panic!("unexpected outcome: {:?}", others),
        }
        assert_eq!(
            lc.latest_block(),
            &Some(block),
            "should update the latest block"
        );
    }

    // Creates a new finality signature for a given block, adds it as pending and returns.
    fn add_pending(
        lc: &mut LinearChain,
        block_hash: BlockHash,
        era_id: EraId,
        local: bool,
    ) -> FinalitySignature {
        let sig = FinalitySignature::random_for_block(block_hash, era_id.value());
        let outcomes = lc.handle_finality_signature(Box::new(sig.clone()), !local);
        assert!(matches!(&*outcomes, [Outcome::LoadSignatures(_)]));
        sig
    }

    // Mark the creator of the signature as bonded.
    fn mark_bonded(lc: &mut LinearChain, fs: FinalitySignature) {
        let outcomes = lc.handle_is_bonded(None, Box::new(fs), true);
        assert!(outcomes.is_empty());
    }

    #[test]
    fn new_block_unvalidated_pending_sigs() {
        let mut rng = TestRng::new();
        let protocol_version = ProtocolVersion::V1_0_0;
        let mut lc = LinearChain::new(protocol_version, 1u64, 1u64);
        let block = Block::random(&mut rng);
        let block_hash = *block.hash();
        let block_era = block.header().era_id();
        // Store some pending finality signatures
        let _sig_a = add_pending(&mut lc, block_hash, block_era, true);
        let _sig_b = add_pending(&mut lc, block_hash, block_era, false);
        let _sig_c = add_pending(&mut lc, block_hash, block_era, false);

        let execution_results = HashMap::new();
        let outcomes = lc.handle_new_block(Box::new(block), execution_results);
        // None of the signatures' creators have been confirmed to be bonded yet.
        // We should not gossip/store/announce any signatures yet.
        assert!(matches!(
            &*outcomes,
            [Outcome::StoreBlock(_), Outcome::StoreExecutionResults(_, _)]
        ));
    }

    #[test]
    fn new_block_bonded_pending_sigs() {
        let mut rng = TestRng::new();
        let protocol_version = ProtocolVersion::V1_0_0;
        let mut lc = LinearChain::new(protocol_version, 1u64, 1u64);
        let block = Block::random(&mut rng);
        let block_hash = *block.hash();
        let block_era = block.header().era_id();
        // Store some pending finality signatures
        let sig_a = add_pending(&mut lc, block_hash, block_era, true);
        let sig_b = add_pending(&mut lc, block_hash, block_era, false);
        let _sig_c = add_pending(&mut lc, block_hash, block_era, false);
        // Mark two of the creators as bonded.
        mark_bonded(&mut lc, sig_a.clone());
        mark_bonded(&mut lc, sig_b.clone());
        let execution_results = HashMap::new();
        let outcomes = lc.handle_new_block(Box::new(block.clone()), execution_results.clone());
        // `sig_a` and `sig_b` are valid and created by bonded validators.
        let expected_outcomes = {
            let mut tmp = vec![];
            let mut block_signatures = BlockSignatures::new(block_hash, block_era);
            block_signatures.insert_proof(sig_a.public_key, sig_a.signature);
            block_signatures.insert_proof(sig_b.public_key, sig_b.signature);
            tmp.push(Outcome::StoreBlockSignatures(block_signatures));
            tmp.push(Outcome::Gossip(Box::new(sig_a.clone()))); // Only `sig_a` was created locally and we don't "regossip" incoming signatures.
            tmp.push(Outcome::AnnounceSignature(Box::new(sig_a)));
            tmp.push(Outcome::AnnounceSignature(Box::new(sig_b)));
            tmp.push(Outcome::StoreBlock(Box::new(block)));
            tmp.push(Outcome::StoreExecutionResults(
                block_hash,
                execution_results,
            ));
            tmp
        };
        // Verify that all outcomes are expected.
        for outcome in outcomes {
            assert!(
                expected_outcomes.iter().any(|&e| e == &outcome),
                "unexpected outcome"
            );
        }
    }

    #[test]
    fn pending_sig_rejected() {
        let mut rng = TestRng::new();
        let protocol_version = ProtocolVersion::V1_0_0;
        let mut lc = LinearChain::new(protocol_version, 1u64, 1u64);
        let block_hash = BlockHash::random(&mut rng);
        let valid_sig = FinalitySignature::random_for_block(block_hash, 0);
        let handle_sig_outcomes = lc.handle_finality_signature(Box::new(valid_sig.clone()), false);
        assert!(matches!(
            &*handle_sig_outcomes,
            &[Outcome::LoadSignatures(_)]
        ));
        assert!(
            lc.handle_finality_signature(Box::new(valid_sig), false)
                .is_empty(),
            "adding already-pending signature should be a no-op"
        );
    }

    // Forces caching of the finality signature. Requires confirming that creator is known to be
    // bonded.
    fn cache_signature(lc: &mut LinearChain, fs: FinalitySignature) {
        // We need to signal that block is known. Otherwise we won't cache the signature.
        let mut block_signatures = BlockSignatures::new(fs.block_hash, fs.era_id);
        let outcomes = lc.handle_cached_signatures(
            Some(Box::new(block_signatures.clone())),
            Box::new(fs.clone()),
        );
        match &*outcomes {
            [Outcome::VerifyIfBonded {
                new_fs, known_fs, ..
            }] => {
                assert_eq!(&fs, &**new_fs);
                let outcomes = lc.handle_is_bonded(known_fs.clone(), Box::new(fs.clone()), true);
                // After confirming that signature is valid and block known, we want to store the
                // signature and announce it.
                match &*outcomes {
                    [Outcome::AnnounceSignature(outcome_fs), Outcome::StoreBlockSignatures(outcome_block_signatures)] =>
                    {
                        assert_eq!(&fs, &**outcome_fs);
                        // LinearChain component will update the `block_signatures` with a new
                        // signature for storage.
                        block_signatures.insert_proof(fs.public_key, fs.signature);
                        assert_eq!(&block_signatures, outcome_block_signatures);
                    }
                    others => panic!("unexpected outcomes {:?}", others),
                }
            }
            others => panic!("unexpected outcomes {:?}", others),
        }
    }

    #[test]
    fn known_sig_rejected() {
        let _ = logging::init();
        let mut rng = TestRng::new();
        let protocol_version = ProtocolVersion::V1_0_0;
        let mut lc = LinearChain::new(protocol_version, 1u64, 1u64);
        let block = Block::random(&mut rng);
        let valid_sig =
            FinalitySignature::random_for_block(*block.hash(), block.header().era_id().value());
        cache_signature(&mut lc, valid_sig.clone());
        let outcomes = lc.handle_finality_signature(Box::new(valid_sig), false);
        assert!(
            outcomes.is_empty(),
            "adding already-known signature should be a no-op"
        );
    }

    #[test]
    fn invalid_sig_rejected() {
        let _ = logging::init();
        let mut rng = TestRng::new();
        let protocol_version = ProtocolVersion::V1_0_0;
        let auction_delay = 1;
        let unbonding_delay = 2;
        let mut lc = LinearChain::new(protocol_version, auction_delay, unbonding_delay);
        // Set the latest known block so that we can trigger the following checks.
        let block = Block::random_with_specifics(&mut rng, EraId::new(3), 10, false);
        let block_hash = *block.hash();
        let block_era = block.header().era_id();
        let put_block_outcomes = lc.handle_put_block(Box::new(block.clone()));
        assert_eq!(put_block_outcomes.len(), 1);
        assert_eq!(
            lc.latest_block(),
            &Some(block),
            "should update the latest block"
        );
        // signature's era either too low or too high
        let era_too_low_sig = FinalitySignature::random_for_block(block_hash, 0);
        let outcomes = lc.handle_finality_signature(Box::new(era_too_low_sig), false);
        assert!(outcomes.is_empty());
        let era_too_high_sig =
            FinalitySignature::random_for_block(block_hash, block_era.value() + auction_delay + 1);
        let outcomes = lc.handle_finality_signature(Box::new(era_too_high_sig), false);
        assert!(outcomes.is_empty());
        // signature is not valid
        let block_hash = BlockHash::random(&mut rng);
        let (_, pub_key) = generate_ed25519_keypair();
        let mut invalid_sig = FinalitySignature::random_for_block(block_hash, block_era.value());
        // replace the public key so that the verification fails.
        invalid_sig.public_key = pub_key;
        let outcomes = lc.handle_finality_signature(Box::new(invalid_sig), false);
        assert!(outcomes.is_empty())
    }
}
