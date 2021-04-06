use datasize::DataSize;
use itertools::Itertools;
use tracing::{debug, warn};

use crate::types::{Block, BlockHash, BlockSignatures, FinalitySignature};
use casper_types::{EraId, ProtocolVersion};

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
    activation_era_id: EraId,
    /// Current protocol version of the network.
    protocol_version: ProtocolVersion,
    auction_delay: u64,
    unbonding_delay: u64,
}

impl LinearChain {
    pub(crate) fn new(
        protocol_version: ProtocolVersion,
        auction_delay: u64,
        unbonding_delay: u64,
        activation_era_id: EraId,
    ) -> Self {
        LinearChain {
            latest_block: None,
            pending_finality_signatures: PendingSignatures::new(),
            signature_cache: SignatureCache::new(),
            activation_era_id,
            protocol_version,
            auction_delay,
            unbonding_delay,
        }
    }

    /// Returns whether we have already enqueued that finality signature.
    pub(super) fn is_pending(&self, fs: &FinalitySignature) -> bool {
        let creator = fs.public_key;
        let block_hash = fs.block_hash;
        self.pending_finality_signatures
            .has_finality_signature(&creator, &block_hash)
    }

    /// Returns whether we have already seen and stored the finality signature.
    pub(super) fn is_new(&self, fs: &FinalitySignature) -> bool {
        !self.signature_cache.known_signature(fs)
    }

    // New linear chain block received. Collect any pending finality signatures that
    // were waiting for that block.
    pub(super) fn new_block(&mut self, block: &Block) -> Vec<Signature> {
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
    pub(super) fn add_pending_finality_signature(
        &mut self,
        fs: FinalitySignature,
        gossiped: bool,
    ) -> bool {
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
    pub(super) fn remove_from_pending_fs(&mut self, fs: &FinalitySignature) -> Option<Signature> {
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
    pub(super) fn cache_signatures(&mut self, mut signatures: BlockSignatures) {
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
    pub(super) fn get_signatures(&self, block_hash: &BlockHash) -> Option<BlockSignatures> {
        self.signature_cache.get(block_hash)
    }

    pub(super) fn current_protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }

    pub(super) fn set_latest_block(&mut self, block: Block) {
        self.latest_block = Some(block);
    }

    pub(super) fn latest_block(&self) -> &Option<Block> {
        &self.latest_block
    }

    /// Returns finality signatures for `block_hash`.
    fn collect_pending_finality_signatures(&mut self, block_hash: &BlockHash) -> Vec<Signature> {
        self.pending_finality_signatures
            .collect_pending(block_hash)
            .into_iter()
            .filter(|sig| !self.signature_cache.known_signature(sig.to_inner()))
            .collect_vec()
    }
}
