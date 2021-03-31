use std::{fmt::Display, marker::PhantomData};

use casper_types::{EraId, ProtocolVersion};
use datasize::DataSize;
use prometheus::Registry;
use tracing::debug;

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
    metrics::LinearChainMetrics, pending_signatures::PendingSignatures, signature::Signature,
    signature_cache::SignatureCache, Event,
};
#[derive(DataSize, Debug)]
pub(crate) struct LinearChain<I> {
    /// The most recently added block.
    pub(super) latest_block: Option<Block>,
    /// Finality signatures to be inserted in a block once it is available.
    pending_finality_signatures: PendingSignatures,
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
            pending_finality_signatures: PendingSignatures::new(),
            signature_cache: SignatureCache::new(),
            activation_era_id,
            protocol_version,
            auction_delay,
            unbonding_delay,
            metrics,
            _marker: PhantomData,
        })
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
            .collect_pending(block_hash, &known_signatures);
        // Add new signatures and send the updated block to storage.
        for signature in pending_sigs {
            if signature.to_inner().era_id != block_era {
                // finality signature was created with era id that doesn't match block's era.
                // TODO: disconnect from the sender.
                continue;
            }
            known_signatures.insert_proof(
                signature.to_inner().public_key,
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
        } = fs;
        // TODO: Verify if we already know that finality signature.
        debug!(%block_hash, %public_key, "received new finality signature");
        let signature = if gossiped {
            Signature::External(Box::new(fs))
        } else {
            Signature::Local(Box::new(fs))
        };
        self.pending_finality_signatures.add(signature);
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

    // Caches the signature.
    pub(super) fn cache_signatures(&mut self, signatures: BlockSignatures) {
        self.signature_cache.insert(signatures);
    }
}
