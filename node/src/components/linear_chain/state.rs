use std::{collections::HashMap, fmt::Display, marker::PhantomData};

use casper_types::{EraId, ProtocolVersion, PublicKey};
use datasize::DataSize;
use itertools::Itertools;
use prometheus::Registry;
use tracing::{debug, warn};

use crate::{
    effect::{
        announcements::LinearChainAnnouncement,
        requests::{NetworkRequest, StorageRequest},
        EffectBuilder, EffectExt, Effects,
    },
    protocol::Message,
    types::{Block, BlockHash, BlockSignatures, FinalitySignature},
};

use super::{
    metrics::LinearChainMetrics, signature::Signature, signature_cache::SignatureCache, Event,
};

/// The maximum number of finality signatures from a single validator we keep in memory while
/// waiting for their block.
const MAX_PENDING_FINALITY_SIGNATURES_PER_VALIDATOR: usize = 1000;
#[derive(DataSize, Debug)]
pub(crate) struct LinearChain<I> {
    /// The most recently added block.
    pub(super) latest_block: Option<Block>,
    /// Finality signatures to be inserted in a block once it is available.
    pending_finality_signatures: HashMap<PublicKey, HashMap<BlockHash, Signature>>,
    pub(super) signature_cache: SignatureCache,
    pub(super) activation_era_id: EraId,
    /// Current protocol version of the network.
    pub(super) protocol_version: ProtocolVersion,
    pub(super) auction_delay: u64,
    pub(super) unbonding_delay: u64,
    #[data_size(skip)]
    pub(super) metrics: LinearChainMetrics,

    _marker: PhantomData<I>,
}

impl<I> LinearChain<I> {
    pub fn new(
        registry: &Registry,
        protocol_version: ProtocolVersion,
        auction_delay: u64,
        unbonding_delay: u64,
        activation_era_id: EraId,
    ) -> Result<Self, prometheus::Error> {
        let metrics = LinearChainMetrics::new(registry)?;
        Ok(LinearChain {
            latest_block: None,
            pending_finality_signatures: HashMap::new(),
            signature_cache: SignatureCache::new(),
            activation_era_id,
            protocol_version,
            auction_delay,
            unbonding_delay,
            metrics,
            _marker: PhantomData,
        })
    }

    // Checks if we have already enqueued that finality signature.
    pub(super) fn has_finality_signature(&self, fs: &FinalitySignature) -> bool {
        let creator = fs.public_key.clone();
        let block_hash = fs.block_hash;
        self.pending_finality_signatures
            .get(&creator)
            .map_or(false, |sigs| sigs.contains_key(&block_hash))
    }

    /// Removes all entries for which there are no finality signatures.
    fn remove_empty_entries(&mut self) {
        self.pending_finality_signatures
            .retain(|_, sigs| !sigs.is_empty());
    }

    /// Adds pending finality signatures to the block; returns events to announce and broadcast
    /// them, and the updated block signatures.
    pub(super) fn collect_pending_finality_signatures<REv>(
        &mut self,
        block_hash: &BlockHash,
        block_era: EraId,
        effect_builder: EffectBuilder<REv>,
    ) -> (BlockSignatures, Effects<Event<I>>)
    where
        REv: From<StorageRequest>
            + From<NetworkRequest<I, Message>>
            + From<LinearChainAnnouncement>
            + Send,
        I: Display + Send + 'static,
    {
        let mut effects = Effects::new();
        let mut known_signatures = self
            .signature_cache
            .get_known_signatures(block_hash, block_era);
        let pending_sigs = self
            .pending_finality_signatures
            .values_mut()
            .filter_map(|sigs| sigs.remove(&block_hash))
            .filter(|signature| {
                !known_signatures
                    .proofs
                    .contains_key(&signature.to_inner().public_key)
            })
            .collect_vec();
        self.remove_empty_entries();
        // Add new signatures and send the updated block to storage.
        for signature in pending_sigs {
            if signature.to_inner().era_id != block_era {
                // finality signature was created with era id that doesn't match block's era.
                // TODO: disconnect from the sender.
                continue;
            }
            known_signatures.insert_proof(
                signature.to_inner().public_key.clone(),
                signature.to_inner().signature,
            );
            if signature.is_local() {
                let message = Message::FinalitySignature(Box::new(signature.to_inner().clone()));
                effects.extend(effect_builder.broadcast_message(message).ignore());
            }
            effects.extend(
                effect_builder
                    .announce_finality_signature(signature.take())
                    .ignore(),
            );
        }
        (known_signatures, effects)
    }

    /// Adds finality signature to the collection of pending finality signatures.
    pub(super) fn add_pending_finality_signature(&mut self, fs: FinalitySignature, gossiped: bool) {
        let FinalitySignature {
            block_hash,
            public_key,
            ..
        } = fs.clone();
        debug!(%block_hash, %public_key, "received new finality signature");
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
            return;
        }

        let signature = if gossiped {
            Signature::External(Box::new(fs))
        } else {
            Signature::Local(Box::new(fs))
        };
        // Add the pending signature.
        let _ = sigs.insert(block_hash, signature);
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
        let maybe_sig =
            if let Some(validator_sigs) = self.pending_finality_signatures.get_mut(public_key) {
                validator_sigs.remove(&block_hash)
            } else {
                None
            };
        self.remove_empty_entries();
        maybe_sig
    }
}
