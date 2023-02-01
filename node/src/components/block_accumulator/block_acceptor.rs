use std::collections::{BTreeMap, BTreeSet};

use datasize::DataSize;
use itertools::Itertools;
use tracing::{debug, error, warn};

use casper_types::{EraId, PublicKey, Timestamp};

use crate::{
    components::{
        block_accumulator::error::{Bogusness, Error as AcceptorError, InvalidGossipError},
        fetcher::{EmptyValidationMetadata, FetchItem},
    },
    types::{
        BlockHash, BlockSignatures, EraValidatorWeights, FinalitySignature, MetaBlock, NodeId,
        SignatureWeight,
    },
};

#[derive(DataSize, Debug)]
pub(super) struct BlockAcceptor {
    block_hash: BlockHash,
    meta_block: Option<MetaBlock>,
    signatures: BTreeMap<PublicKey, (FinalitySignature, BTreeSet<NodeId>)>,
    peers: BTreeSet<NodeId>,
    last_progress: Timestamp,
}

#[derive(Debug, PartialEq)]
#[allow(clippy::large_enum_variant)]
pub(super) enum ShouldStore {
    SufficientlySignedBlock {
        meta_block: MetaBlock,
        block_signatures: BlockSignatures,
    },
    CompletedBlock {
        meta_block: MetaBlock,
        block_signatures: BlockSignatures,
    },
    MarkComplete(MetaBlock),
    SingleSignature(FinalitySignature),
    Nothing,
}

impl BlockAcceptor {
    pub(super) fn new<I>(block_hash: BlockHash, peers: I) -> Self
    where
        I: IntoIterator<Item = NodeId>,
    {
        Self {
            block_hash,
            meta_block: None,
            signatures: BTreeMap::new(),
            peers: peers.into_iter().collect(),
            last_progress: Timestamp::now(),
        }
    }

    pub(super) fn peers(&self) -> &BTreeSet<NodeId> {
        &self.peers
    }

    pub(super) fn register_peer(&mut self, peer: NodeId) {
        self.peers.insert(peer);
    }

    pub(super) fn register_block(
        &mut self,
        meta_block: MetaBlock,
        peer: Option<NodeId>,
    ) -> Result<(), AcceptorError> {
        if self.block_hash() != *meta_block.block.hash() {
            return Err(AcceptorError::BlockHashMismatch {
                expected: self.block_hash(),
                actual: *meta_block.block.hash(),
            });
        }

        if let Err(error) = meta_block.block.validate(&EmptyValidationMetadata) {
            warn!(%error, "received invalid block");
            // TODO[RC]: Consider renaming `InvalidGossip` and/or restructuring the errors
            match peer {
                Some(node_id) => {
                    return Err(AcceptorError::InvalidGossip(Box::new(
                        InvalidGossipError::Block {
                            block_hash: *meta_block.block.hash(),
                            peer: node_id,
                            validation_error: error,
                        },
                    )))
                }
                None => return Err(AcceptorError::InvalidConfiguration),
            }
        }

        if let Some(node_id) = peer {
            self.register_peer(node_id);
        }

        match self.meta_block.take() {
            Some(existing_meta_block) => {
                let merged_meta_block = existing_meta_block.merge(meta_block)?;
                self.meta_block = Some(merged_meta_block);
            }
            None => {
                self.meta_block = Some(meta_block);
            }
        }

        Ok(())
    }

    pub(super) fn register_finality_signature(
        &mut self,
        finality_signature: FinalitySignature,
        peer: Option<NodeId>,
    ) -> Result<Option<FinalitySignature>, AcceptorError> {
        if self.block_hash != finality_signature.block_hash {
            return Err(AcceptorError::BlockHashMismatch {
                expected: self.block_hash,
                actual: finality_signature.block_hash,
            });
        }
        if let Err(error) = finality_signature.is_verified() {
            warn!(%error, "received invalid finality signature");
            match peer {
                Some(node_id) => {
                    return Err(AcceptorError::InvalidGossip(Box::new(
                        InvalidGossipError::FinalitySignature {
                            block_hash: finality_signature.block_hash,
                            peer: node_id,
                            validation_error: error,
                        },
                    )))
                }
                None => return Err(AcceptorError::InvalidConfiguration),
            }
        }

        let had_sufficient_finality = self.has_sufficient_finality();
        // if we don't have finality yet, collect the signature and return
        // while we could store the finality signature, we currently prefer
        // to store block and signatures when sufficient weight is attained
        if false == had_sufficient_finality {
            if let Some(node_id) = peer {
                self.register_peer(node_id);
            }
            self.signatures
                .entry(finality_signature.public_key.clone())
                .and_modify(|(_, senders)| senders.extend(peer))
                .or_insert_with(|| (finality_signature, peer.into_iter().collect()));
            return Ok(None);
        }

        if let Some(meta_block) = &self.meta_block {
            // if the signature's era does not match the block's era
            // it's malicious / bogus / invalid.
            if meta_block.block.header().era_id() != finality_signature.era_id {
                match peer {
                    Some(node_id) => {
                        return Err(AcceptorError::EraMismatch {
                            block_hash: finality_signature.block_hash,
                            expected: meta_block.block.header().era_id(),
                            actual: finality_signature.era_id,
                            peer: node_id,
                        });
                    }
                    None => return Err(AcceptorError::InvalidConfiguration),
                }
            }
        } else {
            // should have block if self.has_sufficient_finality()
            return Err(AcceptorError::SufficientFinalityWithoutBlock {
                block_hash: finality_signature.block_hash,
            });
        }

        if let Some(node_id) = peer {
            self.register_peer(node_id);
        }
        let is_new = !self.signatures.contains_key(&finality_signature.public_key);

        self.signatures
            .entry(finality_signature.public_key.clone())
            .and_modify(|(_, senders)| senders.extend(peer))
            .or_insert_with(|| (finality_signature.clone(), peer.into_iter().collect()));

        if had_sufficient_finality && is_new {
            // we received this finality signature after putting the block & earlier signatures
            // to storage
            self.touch();
            return Ok(Some(finality_signature));
        };

        // either we've seen this signature already or we're still waiting for sufficient finality
        Ok(None)
    }

    /// Returns instructions to write the block and/or finality signatures to storage.
    /// Also returns a set of peers that sent us invalid data and should be banned.
    pub(super) fn should_store_block(
        &mut self,
        era_validator_weights: &EraValidatorWeights,
    ) -> (ShouldStore, Vec<(NodeId, AcceptorError)>) {
        let block_hash = self.block_hash;
        let no_block = self.meta_block.is_none();
        let no_sigs = self.signatures.is_empty();
        if self.has_sufficient_finality() {
            if let Some(meta_block) = self.meta_block.as_mut() {
                if meta_block.state.is_executed()
                    && meta_block.state.register_as_marked_complete().was_updated()
                {
                    debug!(
                        %block_hash, no_block, no_sigs,
                        "already have sufficient finality signatures, but marking block complete"
                    );
                    return (ShouldStore::MarkComplete(meta_block.clone()), Vec::new());
                }
            }

            debug!(
                %block_hash, no_block, no_sigs,
                "not storing anything - already have sufficient finality signatures"
            );
            return (ShouldStore::Nothing, Vec::new());
        }

        if no_block || no_sigs {
            debug!(%block_hash, no_block, no_sigs, "not storing block");
            return (ShouldStore::Nothing, Vec::new());
        }

        let faulty_senders = self.remove_bogus_validators(era_validator_weights);
        let signature_weight = era_validator_weights.signature_weight(self.signatures.keys());
        if SignatureWeight::Strict == signature_weight {
            self.touch();
            if let Some(meta_block) = self.meta_block.as_mut() {
                let mut block_signatures = BlockSignatures::new(
                    *meta_block.block.hash(),
                    meta_block.block.header().era_id(),
                );
                self.signatures.values().for_each(|(signature, _)| {
                    block_signatures
                        .insert_proof(signature.public_key.clone(), signature.signature);
                });
                if meta_block
                    .state
                    .register_has_sufficient_finality()
                    .was_already_registered()
                {
                    error!(
                        %block_hash,
                        block_height = meta_block.block.height(),
                        meta_block_state = ?meta_block.state,
                        "should not register having sufficient finality for the same block more \
                        than once"
                    );
                }
                if meta_block.state.is_executed() {
                    if meta_block
                        .state
                        .register_as_marked_complete()
                        .was_already_registered()
                    {
                        error!(
                            %block_hash,
                            block_height = meta_block.block.height(),
                            meta_block_state = ?meta_block.state,
                            "should not mark the same block complete more than once"
                        );
                    }

                    return (
                        ShouldStore::CompletedBlock {
                            meta_block: meta_block.clone(),
                            block_signatures,
                        },
                        faulty_senders,
                    );
                } else {
                    if meta_block
                        .state
                        .register_as_stored()
                        .was_already_registered()
                    {
                        error!(
                            %block_hash,
                            block_height = meta_block.block.height(),
                            meta_block_state = ?meta_block.state,
                            "should not store the same block more than once"
                        );
                    }
                    return (
                        ShouldStore::SufficientlySignedBlock {
                            meta_block: meta_block.clone(),
                            block_signatures,
                        },
                        faulty_senders,
                    );
                }
            }
        }

        let signed_weight = era_validator_weights.signed_weight(self.signatures.keys());
        let total_era_weight = era_validator_weights.get_total_weight();
        let satisfaction_percent = signed_weight * 100 / total_era_weight;
        debug!(
            %block_hash,
            %signed_weight,
            %total_era_weight,
            %satisfaction_percent,
            no_block, no_sigs,
            "not storing anything - insufficient finality signatures"
        );
        (ShouldStore::Nothing, faulty_senders)
    }

    pub(super) fn has_sufficient_finality(&self) -> bool {
        self.meta_block
            .as_ref()
            .map(|meta_block| meta_block.state.has_sufficient_finality())
            .unwrap_or(false)
    }

    pub(super) fn era_id(&self) -> Option<EraId> {
        if let Some(meta_block) = &self.meta_block {
            return Some(meta_block.block.header().era_id());
        }
        if let Some((finality_signature, _)) = self.signatures.values().next() {
            return Some(finality_signature.era_id);
        }
        None
    }

    pub(super) fn block_height(&self) -> Option<u64> {
        self.meta_block
            .as_ref()
            .map(|meta_block| meta_block.block.header().height())
    }

    pub(super) fn block_hash(&self) -> BlockHash {
        self.block_hash
    }

    pub(super) fn last_progress(&self) -> Timestamp {
        self.last_progress
    }

    /// Removes finality signatures that have the wrong era ID or are signed by non-validators.
    /// Returns the set of peers that sent us these signatures.
    fn remove_bogus_validators(
        &mut self,
        era_validator_weights: &EraValidatorWeights,
    ) -> Vec<(NodeId, AcceptorError)> {
        let bogus_validators = era_validator_weights.bogus_validators(self.signatures.keys());

        let mut faulty_senders = Vec::new();
        bogus_validators.iter().for_each(|bogus_validator| {
            debug!(%bogus_validator, "bogus validator");
            if let Some((_, senders)) = self.signatures.remove(bogus_validator) {
                faulty_senders.extend(senders.iter().map(|sender| {
                    (
                        *sender,
                        AcceptorError::BogusValidator(Bogusness::NotAValidator),
                    )
                }));
            }
        });

        if let Some(meta_block) = &self.meta_block {
            let bogus_validators = self
                .signatures
                .iter()
                .filter(|(_, (v, _))| v.era_id != meta_block.block.header().era_id())
                .map(|(k, _)| k.clone())
                .collect_vec();

            bogus_validators.iter().for_each(|bogus_validator| {
                debug!(%bogus_validator, "bogus validator");
                if let Some((_, senders)) = self.signatures.remove(bogus_validator) {
                    faulty_senders.extend(senders.iter().map(|sender| {
                        (
                            *sender,
                            AcceptorError::BogusValidator(Bogusness::SignatureEraIdMismatch),
                        )
                    }));
                }
            });
        }

        for (node_id, _) in &faulty_senders {
            self.peers.remove(node_id);
        }

        faulty_senders
    }

    fn touch(&mut self) {
        self.last_progress = Timestamp::now();
    }
}

#[cfg(test)]
impl BlockAcceptor {
    pub(super) fn executed(&self) -> bool {
        self.meta_block
            .as_ref()
            .map_or(false, |meta_block| meta_block.state.is_executed())
    }

    pub(super) fn meta_block(&self) -> Option<MetaBlock> {
        self.meta_block.clone()
    }

    pub(super) fn set_last_progress(&mut self, last_progress: Timestamp) {
        self.last_progress = last_progress;
    }

    pub(super) fn set_meta_block(&mut self, meta_block: Option<MetaBlock>) {
        self.meta_block = meta_block;
    }

    pub(super) fn set_sufficient_finality(&mut self, has_sufficient_finality: bool) {
        if let Some(meta_block) = self.meta_block.as_mut() {
            meta_block
                .state
                .set_sufficient_finality(has_sufficient_finality);
        }
    }

    pub(super) fn signatures(&self) -> &BTreeMap<PublicKey, (FinalitySignature, BTreeSet<NodeId>)> {
        &self.signatures
    }

    pub(super) fn signatures_mut(
        &mut self,
    ) -> &mut BTreeMap<PublicKey, (FinalitySignature, BTreeSet<NodeId>)> {
        &mut self.signatures
    }
}
