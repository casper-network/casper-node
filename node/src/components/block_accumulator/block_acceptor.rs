use std::collections::{BTreeMap, BTreeSet};

use datasize::DataSize;
use itertools::Itertools;
use tracing::{debug, warn};

use casper_types::{EraId, PublicKey, Timestamp};

use crate::{
    components::block_accumulator::error::{Bogusness, Error as AcceptorError, InvalidGossipError},
    types::{
        Block, BlockHash, EmptyValidationMetadata, EraValidatorWeights, FetcherItem,
        FinalitySignature, NodeId, SignatureWeight,
    },
};

#[derive(DataSize, Debug)]
pub(super) struct BlockAcceptor {
    block_hash: BlockHash,
    block: Option<Block>,
    signatures: BTreeMap<PublicKey, (FinalitySignature, BTreeSet<NodeId>)>,
    peers: BTreeSet<NodeId>,
    has_sufficient_finality: bool,
    last_progress: Timestamp,
    executed: bool,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub(super) enum ShouldStore {
    SufficientlySignedBlock {
        block: Block,
        signatures: Vec<FinalitySignature>,
        executed: bool,
    },
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
            block: None,
            signatures: BTreeMap::new(),
            peers: peers.into_iter().collect(),
            has_sufficient_finality: false,
            last_progress: Timestamp::now(),
            executed: false,
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
        block: Block,
        peer: Option<NodeId>,
        executed: bool,
    ) -> Result<(), AcceptorError> {
        if self.block_hash() != *block.hash() {
            return Err(AcceptorError::BlockHashMismatch {
                expected: self.block_hash(),
                actual: *block.hash(),
            });
        }

        if let Err(error) = block.validate(&EmptyValidationMetadata) {
            warn!(%error, "received invalid block");
            // TODO[RC]: Consider renaming `InvalidGossip` and/or restructuring the errors
            match peer {
                Some(node_id) => {
                    return Err(AcceptorError::InvalidGossip(Box::new(
                        InvalidGossipError::Block {
                            block_hash: *block.hash(),
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

        if self.block.is_none() {
            self.block = Some(block);
        }

        self.executed |= executed;

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

        let had_sufficient_finality = self.has_sufficient_finality;
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

        if let Some(block) = &self.block {
            // if the signature's era does not match the block's era
            // it's malicious / bogus / invalid.
            if block.header().era_id() != finality_signature.era_id {
                match peer {
                    Some(node_id) => {
                        return Err(AcceptorError::EraMismatch {
                            block_hash: finality_signature.block_hash,
                            expected: block.header().era_id(),
                            actual: finality_signature.era_id,
                            peer: node_id,
                        });
                    }
                    None => return Err(AcceptorError::InvalidConfiguration),
                }
            }
        } else {
            // should have block if self.has_sufficient_finality
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
        if self.has_sufficient_finality {
            debug!(
                block_hash = %self.block_hash,
                no_block = self.block.is_none(),
                no_sigs = self.signatures.is_empty(),
                "not storing anything - already have sufficient finality signatures"
            );
            return (ShouldStore::Nothing, Vec::new());
        }

        if self.block.is_none() || self.signatures.is_empty() {
            debug!(
                block_hash = %self.block_hash,
                no_block = self.block.is_none(),
                no_sigs = self.signatures.is_empty(),
                "not storing block"
            );
            return (ShouldStore::Nothing, Vec::new());
        }

        let faulty_senders = self.remove_bogus_validators(era_validator_weights);
        if SignatureWeight::Sufficient
            == era_validator_weights.has_sufficient_weight(self.signatures.keys())
        {
            if let Some(block) = self.block.clone() {
                self.touch();
                self.has_sufficient_finality = true;
                return (
                    ShouldStore::SufficientlySignedBlock {
                        block,
                        signatures: self
                            .signatures
                            .values()
                            .map(|(sig, _)| sig.clone())
                            .collect_vec(),
                        executed: self.executed,
                    },
                    faulty_senders,
                );
            }
        }

        debug!(
            block_hash = %self.block_hash,
            no_block = self.block.is_none(),
            no_sigs = self.signatures.is_empty(),
            "not storing anything - insufficient finality signatures"
        );
        (ShouldStore::Nothing, faulty_senders)
    }

    pub(super) fn has_sufficient_finality(&self) -> bool {
        self.has_sufficient_finality
    }

    pub(super) fn era_id(&self) -> Option<EraId> {
        if let Some(block) = &self.block {
            return Some(block.header().era_id());
        }
        if let Some((finality_signature, _)) = self.signatures.values().next() {
            return Some(finality_signature.era_id);
        }
        None
    }

    pub(super) fn block_height(&self) -> Option<u64> {
        self.block.as_ref().map(|block| block.header().height())
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

        if let Some(block) = &self.block {
            let bogus_validators = self
                .signatures
                .iter()
                .filter(|(_, (v, _))| v.era_id != block.header().era_id())
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
