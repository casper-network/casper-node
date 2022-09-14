use std::collections::HashMap;

use datasize::DataSize;
use itertools::Itertools;
use num::rational::Ratio;
use tracing::{debug, warn};

use casper_hashing::Digest;
use casper_types::{EraId, ExecutionResult, ProtocolVersion};

use super::{
    pending_signatures::PendingSignatures, signature::Signature, signature_cache::SignatureCache,
};
use crate::{
    components::{
        chain_synchronizer::KeyBlockInfo,
        linear_chain::{self, BlockSignatureError},
    },
    types::{ActivationPoint, Block, BlockHash, BlockSignatures, DeployHash, FinalitySignature},
};

#[derive(DataSize, Debug)]
pub(crate) struct LinearChain {
    /// The most recently added block.
    latest_block: Option<Block>,
    /// The key block information of the most recent eras.
    key_block_info: HashMap<EraId, KeyBlockInfo>,
    /// Finality signatures to be inserted in a block once it is available.
    pending_finality_signatures: PendingSignatures,
    signature_cache: SignatureCache,
    /// Current protocol version of the network.
    protocol_version: ProtocolVersion,
    auction_delay: u64,
    unbonding_delay: u64,
    /// The fraction of validators, by weight, that have to sign a block to prove its finality.
    #[data_size(skip)]
    finality_threshold_fraction: Ratio<u64>,
    /// The next upgrade activation point. When the key block for this era is fully signed, the
    /// linear chain component indicates that the node should restart for an upgrade.
    next_upgrade_activation_point: Option<ActivationPoint>,
}

#[derive(Debug, Eq, PartialEq)]
pub(super) enum Outcome {
    // Store block signatures to storage. If the flag is `true` this completes the signatures for
    // the last block before an upgrade.
    StoreBlockSignatures(BlockSignatures, bool),
    // Store block and execution results.
    StoreBlock {
        block: Box<Block>,
        approvals_checksum: Digest,
        execution_results_checksum: Digest,
        execution_results: HashMap<DeployHash, ExecutionResult>,
    },
    // Read finality signatures for the block from storage.
    LoadSignatures(Box<FinalitySignature>),
    // Gossip finality signature to peers.
    Gossip(Box<FinalitySignature>),
    // Create a reactor announcement about new (valid) finality signatures.
    AnnounceSignature(Box<FinalitySignature>),
    // Create a reactor announcement about new (valid) block.
    AnnounceBlock {
        approvals_checksum: Digest,
        execution_results_checksum: Digest,
        block: Box<Block>,
    },
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
        finality_threshold_fraction: Ratio<u64>,
        next_upgrade_activation_point: Option<ActivationPoint>,
    ) -> Self {
        LinearChain {
            latest_block: None,
            key_block_info: Default::default(),
            pending_finality_signatures: PendingSignatures::new(),
            signature_cache: SignatureCache::new(),
            protocol_version,
            auction_delay,
            unbonding_delay,
            next_upgrade_activation_point,
            finality_threshold_fraction,
        }
    }

    /// Handles registering an upgrade activation point.
    pub(super) fn got_upgrade_activation_point(&mut self, activation_point: ActivationPoint) {
        debug!(?activation_point, "got an activation point");
        self.next_upgrade_activation_point = Some(activation_point);
    }

    /// Returns whether we have already enqueued that finality signature.
    fn is_pending(&self, fs: &FinalitySignature) -> bool {
        let creator = fs.public_key.clone();
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
        let mut block_signatures = BlockSignatures::new(*block.hash(), block.header().era_id());
        for sig in signatures.iter() {
            block_signatures.insert_proof(sig.public_key(), sig.signature());
        }
        // Cache the signatures as we expect more finality signatures for the new block to
        // arrive soon.
        // If `signatures` was empty, it will serve as a flag that the `block` is already
        // finalized/known and any incoming signatures should be accepted (given they're
        // from a bonded validator and valid).
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
        } = fs.clone();
        if let Some(latest_block) = self.latest_block.as_ref() {
            let current_era = latest_block.header().next_block_era_id();
            if era_id < self.lowest_acceptable_era_id(current_era)
                || era_id > self.highest_acceptable_era_id(current_era)
            {
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
        if let Err(err) = fs.is_verified() {
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
        debug!(%fs.block_hash, %fs.public_key, "removing finality signature from pending collection");
        self.pending_finality_signatures
            .remove(&fs.public_key, &fs.block_hash)
    }

    /// Caches the signature.
    fn cache_signatures(&mut self, mut signatures: BlockSignatures) {
        // Merge already-known signatures and the new ones.
        self.get_signatures(&signatures.block_hash)
            .iter()
            .for_each(|bs| {
                for (pk, sig) in bs.proofs.iter() {
                    signatures.insert_proof((*pk).clone(), *sig);
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

    fn latest_block(&self) -> &Option<Block> {
        &self.latest_block
    }

    fn lowest_acceptable_era_id(&self, current_era: EraId) -> EraId {
        (current_era + self.auction_delay).saturating_sub(self.unbonding_delay)
    }

    fn highest_acceptable_era_id(&self, current_era: EraId) -> EraId {
        current_era + self.auction_delay
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
        approvals_checksum: Digest,
        execution_results_checksum: Digest,
        execution_results: HashMap<DeployHash, ExecutionResult>,
    ) -> Outcomes {
        vec![Outcome::StoreBlock {
            block,
            approvals_checksum,
            execution_results_checksum,
            execution_results,
        }]
    }

    pub(super) fn handle_put_block(
        &mut self,
        block: Box<Block>,
        approvals_checksum: Digest,
        execution_results_checksum: Digest,
    ) -> Outcomes {
        let mut outcomes = Vec::new();
        let signatures = self.new_block(&*block);
        self.latest_block = Some(*block.clone());
        if let Some(key_block_info) = KeyBlockInfo::maybe_from_block_header(block.header()) {
            let current_era = key_block_info.era_id();
            self.key_block_info.insert(current_era, key_block_info);
            if let Some(old_era_id) = self.lowest_acceptable_era_id(current_era).checked_sub(1) {
                self.key_block_info.remove(&old_era_id);
            }
        }
        if !signatures.is_empty() {
            let mut block_signatures = BlockSignatures::new(*block.hash(), block.header().era_id());
            for sig in signatures.iter() {
                block_signatures.insert_proof(sig.public_key(), sig.signature());
            }
            let should_upgrade = self.should_upgrade(&block_signatures);
            outcomes.push(Outcome::StoreBlockSignatures(
                block_signatures,
                should_upgrade,
            ));
            for signature in signatures {
                if signature.is_local() {
                    outcomes.push(Outcome::Gossip(Box::new(signature.to_inner().clone())));
                }
                outcomes.push(Outcome::AnnounceSignature(signature.take()));
            }
        };
        outcomes.push(Outcome::AnnounceBlock {
            block,
            approvals_checksum,
            execution_results_checksum,
        });
        outcomes
    }

    fn should_upgrade(&self, signatures: &BlockSignatures) -> bool {
        let signed_kb_info = match self
            .key_block_info
            .values()
            .find(|kb_info| *kb_info.block_hash() == signatures.block_hash)
        {
            None => return false, // The signed block is not a key block.
            Some(signed_kb_info) => signed_kb_info,
        };
        if self.next_upgrade_activation_point
            != Some(ActivationPoint::EraId(signed_kb_info.era_id()))
        {
            return false; // This is not the next activation point.
        }
        let era_kb_info = match self.key_block_info.get(&signatures.era_id) {
            None => {
                warn!(?signed_kb_info, "missing previous key block info");
                return false;
            }
            Some(era_kb_info) => era_kb_info,
        };
        matches!(
            linear_chain::check_sufficient_block_signatures(
                era_kb_info.validator_weights(),
                self.finality_threshold_fraction,
                Some(signatures),
            ),
            Ok(())
        )
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
            // We know about the block but we haven't seen any signatures for it yet.
            Some(signatures) if signatures.proofs.is_empty() => vec![Outcome::LoadSignatures(fs)],
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
        if let Some(key_block_info) = self.key_block_info.get(&fs.era_id) {
            let is_bonded = key_block_info
                .validator_weights()
                .contains_key(&fs.public_key);
            return self.handle_is_bonded(signatures, fs, is_bonded);
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
            .mark_bonded(new_fs.public_key.clone(), new_fs.block_hash);

        match maybe_known_signatures
            .or_else(|| self.get_signatures(&new_fs.block_hash).map(Box::new))
        {
            None => {
                // Unknown block but validator is bonded.
                // We should finalize the same block eventually. Either in this or in the
                // next eras. New signature is already cached for later.
                vec![]
            }
            Some(mut known_signatures) => {
                // New finality signature from a bonded validator.
                known_signatures.insert_proof(new_fs.public_key.clone(), new_fs.signature);
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
                let should_upgrade = self.should_upgrade(&*known_signatures);
                outcomes.push(Outcome::StoreBlockSignatures(
                    *known_signatures,
                    should_upgrade,
                ));
                outcomes
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, fmt::Debug, iter};

    use rand::Rng;

    use casper_types::{
        crypto::generate_ed25519_keypair, testing::TestRng, EraId, PublicKey, SecretKey,
    };

    use crate::{logging, types::FinalizedBlock};

    use super::*;

    #[test]
    fn new_block_no_sigs() {
        let mut rng = TestRng::new();
        let protocol_version = ProtocolVersion::V1_0_0;
        let mut lc = LinearChain::new(protocol_version, 1u64, 1u64, Ratio::new(1, 3), None);
        let block = Block::random(&mut rng);
        let approvals_checksum = Digest::hash(vec![rng.gen::<u8>()]);
        let execution_results_checksum = Digest::hash(vec![rng.gen::<u8>()]);
        let execution_results = HashMap::new();
        let new_block_outcomes = lc.handle_new_block(
            Box::new(block.clone()),
            approvals_checksum,
            execution_results_checksum,
            execution_results.clone(),
        );
        match &*new_block_outcomes {
            [Outcome::StoreBlock {
                block: outcome_block,
                approvals_checksum: outcome_approvals_checksum,
                execution_results_checksum: outcome_execution_results_checksum,
                execution_results: outcome_execution_results,
            }] => {
                assert_eq!(&**outcome_block, &block);
                assert_eq!(outcome_approvals_checksum, &approvals_checksum);
                assert_eq!(
                    outcome_execution_results_checksum,
                    &execution_results_checksum
                );
                assert_eq!(outcome_execution_results, &execution_results);
            }
            others => panic!("unexpected outcome: {:?}", others),
        }

        let block_stored_outcomes = lc.handle_put_block(
            Box::new(block.clone()),
            approvals_checksum,
            execution_results_checksum,
        );
        match &*block_stored_outcomes {
            [Outcome::AnnounceBlock {
                block: announced_block,
                approvals_checksum: announced_approvals_checksum,
                execution_results_checksum: announced_execution_results_checksum,
            }] => {
                assert_eq!(&**announced_block, &block);
                assert_eq!(*announced_approvals_checksum, approvals_checksum);
                assert_eq!(
                    *announced_execution_results_checksum,
                    execution_results_checksum
                );
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
    fn fix_em() {
        todo!("fix the following tests")
    }

    /*

    #[test]
    fn new_block_unvalidated_pending_sigs() {
        let mut rng = TestRng::new();
        let protocol_version = ProtocolVersion::V1_0_0;
        let mut lc = LinearChain::new(protocol_version, 1u64, 1u64, Ratio::new(1, 3), None);
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
        assert!(matches!(&*outcomes, [Outcome::StoreBlock { .. }]));
    }

    // Check that `left` is a subset of `right`.
    // i.e. that all elements from `left` exist in `right`.
    fn verify_is_subset<T>(left: &[T], right: &[T])
    where
        T: PartialEq + Eq + Debug,
    {
        for l in left {
            assert!(right.iter().any(|r| r == l), "{:?} not found", l)
        }
    }

    fn assert_equal<T>(l: Vec<T>, r: Vec<T>)
    where
        T: PartialEq + Eq + Debug,
    {
        verify_is_subset(&l, &r);
        verify_is_subset(&r, &l);
    }

    #[test]
    fn new_block_bonded_pending_sigs() {
        let mut rng = TestRng::new();
        let protocol_version = ProtocolVersion::V1_0_0;
        let mut lc = LinearChain::new(protocol_version, 1u64, 1u64, Ratio::new(1, 3), None);
        let block = Box::new(Block::random(&mut rng));
        let block_hash = *block.hash();
        let block_era = block.header().era_id();
        // Store some pending finality signatures
        let sig_a = add_pending(&mut lc, block_hash, block_era, true);
        let sig_b = add_pending(&mut lc, block_hash, block_era, false);
        let sig_c = add_pending(&mut lc, block_hash, block_era, true);
        // Mark two of the creators as bonded.
        mark_bonded(&mut lc, sig_a.clone());
        mark_bonded(&mut lc, sig_b.clone());
        let execution_results = HashMap::new();
        let outcomes = lc.handle_new_block(block.clone(), execution_results.clone());
        assert_equal(
            vec![Outcome::StoreBlock(block.clone(), execution_results)],
            outcomes,
        );

        let outcomes = lc.handle_put_block(block.clone());
        // `sig_a` and `sig_b` are valid and created by bonded validators.
        let expected_outcomes = {
            let mut tmp = vec![];
            let mut block_signatures = BlockSignatures::new(block_hash, block_era);
            block_signatures.insert_proof(sig_a.public_key.clone(), sig_a.signature);
            block_signatures.insert_proof(sig_b.public_key.clone(), sig_b.signature);
            tmp.push(Outcome::StoreBlockSignatures(block_signatures, false));
            // Only `sig_a` was created locally and we don't "regossip" incoming signatures.
            tmp.push(Outcome::Gossip(Box::new(sig_a.clone())));
            tmp.push(Outcome::AnnounceSignature(Box::new(sig_a.clone())));
            tmp.push(Outcome::AnnounceSignature(Box::new(sig_b.clone())));
            tmp.push(Outcome::AnnounceBlock(block));
            tmp
        };
        // Verify that all outcomes are expected.
        assert_equal(expected_outcomes, outcomes);
        // Simulate that the `IsValidatorBonded` event for `sig_c` just arrived.
        // When it was created, there was no `known_signatures` yet.
        let outcomes = lc.handle_is_bonded(None, Box::new(sig_c.clone()), true);
        #[allow(clippy::vec_init_then_push)]
        let expected_outcomes = {
            let mut tmp = vec![];
            tmp.push(Outcome::AnnounceSignature(Box::new(sig_c.clone())));
            tmp.push(Outcome::Gossip(Box::new(sig_c.clone())));
            let mut block_signatures = BlockSignatures::new(block_hash, block_era);
            block_signatures.insert_proof(sig_a.public_key.clone(), sig_a.signature);
            block_signatures.insert_proof(sig_b.public_key.clone(), sig_b.signature);
            block_signatures.insert_proof(sig_c.public_key.clone(), sig_c.signature);
            tmp.push(Outcome::StoreBlockSignatures(block_signatures, false));
            tmp
        };
        assert_equal(expected_outcomes, outcomes);
    }

    #[test]
    fn pending_sig_rejected() {
        let mut rng = TestRng::new();
        let protocol_version = ProtocolVersion::V1_0_0;
        let mut lc = LinearChain::new(protocol_version, 1u64, 1u64, Ratio::new(1, 3), None);
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
                    [Outcome::AnnounceSignature(outcome_fs), Outcome::StoreBlockSignatures(outcome_block_signatures, false)] =>
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
        let mut lc = LinearChain::new(protocol_version, 1u64, 1u64, Ratio::new(1, 3), None);
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
        let mut lc = LinearChain::new(
            protocol_version,
            auction_delay,
            unbonding_delay,
            Ratio::new(1, 3),
            None,
        );

        // Set the latest known block so that we can trigger the following checks.
        let block = Block::random_with_specifics(
            &mut rng,
            EraId::new(3),
            10,
            ProtocolVersion::V1_0_0,
            false,
            None,
        );
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

    #[test]
    fn new_block_then_own_sig() {
        let _ = logging::init();
        let mut rng = TestRng::new();
        let protocol_version = ProtocolVersion::V1_0_0;
        let auction_delay = 1;
        let unbonding_delay = 2;
        let mut lc = LinearChain::new(
            protocol_version,
            auction_delay,
            unbonding_delay,
            Ratio::new(1, 3),
            None,
        );

        // Set the latest known block so that we can trigger the following checks.
        let block = Box::new(Block::random_with_specifics(
            &mut rng,
            EraId::new(3),
            10,
            ProtocolVersion::V1_0_0,
            false,
            None,
        ));
        let block_hash = *block.hash();
        let block_era = block.header().era_id();
        let new_block_outcomes = lc.handle_new_block(block.clone(), HashMap::new());
        let expected_outcomes = vec![Outcome::StoreBlock(block.clone(), HashMap::new())];
        assert_equal(expected_outcomes, new_block_outcomes);

        let put_block_outcomes = lc.handle_put_block(block.clone());
        // Verify that all outcomes are expected.
        assert_equal(vec![Outcome::AnnounceBlock(block)], put_block_outcomes);
        let valid_sig = FinalitySignature::random_for_block(block_hash, block_era.value());
        let outcomes = lc.handle_finality_signature(Box::new(valid_sig.clone()), false);
        assert!(matches!(&*outcomes, [Outcome::LoadSignatures(_)]));
        let cached_sigs_outcomes = lc.handle_cached_signatures(None, Box::new(valid_sig.clone()));
        assert!(matches!(
            &*cached_sigs_outcomes,
            [Outcome::VerifyIfBonded { .. }]
        ));
        let outcomes = lc.handle_is_bonded(None, Box::new(valid_sig.clone()), true);
        assert!(!outcomes.is_empty(), "{:?} should not be empty", outcomes);
        let expected_outcomes = {
            let mut block_signatures = BlockSignatures::new(block_hash, block_era);
            block_signatures.insert_proof(valid_sig.public_key.clone(), valid_sig.signature);
            vec![
                Outcome::StoreBlockSignatures(block_signatures, false),
                Outcome::Gossip(Box::new(valid_sig.clone())),
                Outcome::AnnounceSignature(Box::new(valid_sig)),
            ]
        };
        // Verify that all outcomes are expected.
        assert_equal(expected_outcomes, outcomes);
    }

    #[test]
    fn upgrade_when_fully_signed() {
        let _ = logging::init();
        let mut rng = TestRng::new();
        let protocol_version = ProtocolVersion::V1_0_0;
        let auction_delay = 1;
        let unbonding_delay = 2;

        // The next upgrade is scheduled for era 3.
        let mut lc = LinearChain::new(
            protocol_version,
            auction_delay,
            unbonding_delay,
            Ratio::new(1, 3),
            Some(ActivationPoint::EraId(3.into())),
        );

        // We have four validators, all with the same weight.
        let secret_keys: Vec<SecretKey> = iter::repeat_with(|| {
            SecretKey::ed25519_from_bytes(rng.gen::<[u8; SecretKey::ED25519_LENGTH]>()).unwrap()
        })
        .take(4)
        .collect();
        let validators: BTreeMap<_, _> = secret_keys
            .iter()
            .map(|sk| (PublicKey::from(sk), 100.into()))
            .collect();

        // The switch block in era 1 defines how many validators need to sign the one in era 2.
        let block = Box::new(
            Block::new(
                BlockHash::random(&mut rng),              // parent hash
                rng.gen::<[u8; Digest::LENGTH]>().into(), // parent seed
                rng.gen::<[u8; Digest::LENGTH]>().into(), // state root hash
                FinalizedBlock::random_with_specifics(&mut rng, EraId::from(1), 10, true, None),
                Some(validators.clone()),
                protocol_version,
            )
            .unwrap(),
        );

        let outcomes = lc.handle_put_block(block.clone());
        assert_equal(vec![Outcome::AnnounceBlock(block)], outcomes);

        // The switch block in era 2 is the last before the upgrade.
        let block = Box::new(
            Block::new(
                BlockHash::random(&mut rng),              // parent hash
                rng.gen::<[u8; Digest::LENGTH]>().into(), // parent seed
                rng.gen::<[u8; Digest::LENGTH]>().into(), // state root hash
                FinalizedBlock::random_with_specifics(&mut rng, EraId::from(2), 20, true, None),
                Some(validators),
                protocol_version,
            )
            .unwrap(),
        );

        let era_id = block.header().era_id();
        let signatures: Vec<_> = secret_keys
            .iter()
            .map(|sk| {
                let pk = PublicKey::from(sk);
                Box::new(FinalitySignature::create(*block.hash(), era_id, sk, pk))
            })
            .collect();

        // The first signature has already been added.
        let mut stored_sigs = Box::new(BlockSignatures::new(*block.hash(), era_id));
        assert_equal(
            vec![Outcome::LoadSignatures(signatures[0].clone())],
            lc.handle_finality_signature(signatures[0].clone(), true),
        );
        assert_equal(
            vec![],
            lc.handle_cached_signatures(None, signatures[0].clone()),
        );
        stored_sigs.insert_proof(signatures[0].public_key.clone(), signatures[0].signature);

        // When the block gets finalized, the first signature also gets announced.
        assert_equal(
            vec![
                Outcome::AnnounceBlock(block.clone()),
                Outcome::AnnounceSignature(signatures[0].clone()),
                Outcome::StoreBlockSignatures(*stored_sigs.clone(), false),
            ],
            lc.handle_put_block(block),
        );

        // Two signatures is not enough for an upgrade yet: The upgrade flag is false.
        let outcomes =
            lc.handle_cached_signatures(Some(stored_sigs.clone()), signatures[1].clone());
        stored_sigs.insert_proof(signatures[1].public_key.clone(), signatures[1].signature);
        assert_equal(
            vec![
                Outcome::AnnounceSignature(signatures[1].clone()),
                Outcome::StoreBlockSignatures(*stored_sigs.clone(), false),
            ],
            outcomes,
        );

        // With the third signature the switch block is signed by more than 67%: The flag is true.
        let outcomes =
            lc.handle_cached_signatures(Some(stored_sigs.clone()), signatures[2].clone());
        stored_sigs.insert_proof(signatures[2].public_key.clone(), signatures[2].signature);
        assert_equal(
            vec![
                Outcome::AnnounceSignature(signatures[2].clone()),
                Outcome::StoreBlockSignatures(*stored_sigs, true),
            ],
            outcomes,
        );
    }

    #[test]
    fn upgrade_when_fully_signed_and_got_signatures_before_block() {
        let _ = logging::init();
        let mut rng = TestRng::new();
        let protocol_version = ProtocolVersion::V1_0_0;
        let auction_delay = 1;
        let unbonding_delay = 2;

        // The next upgrade is scheduled for era 3.
        let mut lc = LinearChain::new(
            protocol_version,
            auction_delay,
            unbonding_delay,
            Ratio::new(1, 3),
            Some(ActivationPoint::EraId(3.into())),
        );

        // We have four validators, all with the same weight.
        let secret_keys: Vec<SecretKey> = iter::repeat_with(|| {
            SecretKey::ed25519_from_bytes(rng.gen::<[u8; SecretKey::ED25519_LENGTH]>()).unwrap()
        })
        .take(4)
        .collect();
        let validators: BTreeMap<_, _> = secret_keys
            .iter()
            .map(|sk| (PublicKey::from(sk), 100.into()))
            .collect();

        // The switch block in era 1 defines how many validators need to sign the one in era 2.
        let block = Box::new(
            Block::new(
                BlockHash::random(&mut rng),              // parent hash
                rng.gen::<[u8; Digest::LENGTH]>().into(), // parent seed
                rng.gen::<[u8; Digest::LENGTH]>().into(), // state root hash
                FinalizedBlock::random_with_specifics(&mut rng, EraId::from(1), 10, true, None),
                Some(validators.clone()),
                protocol_version,
            )
            .unwrap(),
        );
        let outcomes = lc.handle_put_block(block.clone());
        assert_equal(vec![Outcome::AnnounceBlock(block)], outcomes);

        // The switch block in era 2 is the last before the upgrade.
        let block = Box::new(
            Block::new(
                BlockHash::random(&mut rng),              // parent hash
                rng.gen::<[u8; Digest::LENGTH]>().into(), // parent seed
                rng.gen::<[u8; Digest::LENGTH]>().into(), // state root hash
                FinalizedBlock::random_with_specifics(&mut rng, EraId::from(2), 20, true, None),
                Some(validators),
                protocol_version,
            )
            .unwrap(),
        );

        let era_id = block.header().era_id();
        let signatures: Vec<_> = secret_keys
            .iter()
            .map(|sk| {
                let pk = PublicKey::from(sk);
                Box::new(FinalitySignature::create(*block.hash(), era_id, sk, pk))
            })
            .collect();

        let mut expected_sigs = BlockSignatures::new(*block.hash(), era_id);
        for fs in &signatures {
            assert_equal(
                vec![Outcome::LoadSignatures(fs.clone())],
                lc.handle_finality_signature(fs.clone(), true),
            );
            assert_equal(vec![], lc.handle_cached_signatures(None, fs.clone()));
            expected_sigs.insert_proof(fs.public_key.clone(), fs.signature);
        }

        let outcomes = lc.handle_put_block(block.clone());
        assert_equal(
            vec![
                Outcome::AnnounceBlock(block),
                Outcome::AnnounceSignature(signatures[0].clone()),
                Outcome::AnnounceSignature(signatures[1].clone()),
                Outcome::AnnounceSignature(signatures[2].clone()),
                Outcome::AnnounceSignature(signatures[3].clone()),
                Outcome::StoreBlockSignatures(expected_sigs, true),
            ],
            outcomes,
        );
    }
     */
}
