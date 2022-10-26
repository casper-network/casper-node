use std::collections::BTreeMap;

use datasize::DataSize;
use itertools::Itertools;
use tracing::{debug, warn};

use casper_types::{EraId, PublicKey, Timestamp};

use crate::{
    components::block_accumulator::error::{
        EraMismatchError, Error as AcceptorError, InvalidGossipError,
    },
    types::{
        Block, BlockHash, EmptyValidationMetadata, EraValidatorWeights, FetcherItem,
        FinalitySignature, NodeId, SignatureWeight,
    },
};

#[derive(DataSize, Debug)]
pub(super) struct BlockAcceptor {
    block_hash: BlockHash,
    block: Option<Block>,
    signatures: BTreeMap<PublicKey, FinalitySignature>,
    peers: Vec<NodeId>,
    has_sufficient_finality: bool,
    last_progress: Timestamp,
}

#[derive(Debug)]
#[allow(clippy::large_enum_variant)]
pub(super) enum ShouldStore {
    SufficientlySignedBlock {
        block: Block,
        signatures: Vec<FinalitySignature>,
    },
    SingleSignature(FinalitySignature),
    Nothing,
}

impl BlockAcceptor {
    pub(super) fn new(block_hash: BlockHash, peers: Vec<NodeId>) -> Self {
        Self {
            block_hash,
            block: None,
            signatures: BTreeMap::new(),
            peers,
            has_sufficient_finality: false,
            last_progress: Timestamp::now(),
        }
    }

    pub(super) fn peers(&self) -> Vec<NodeId> {
        self.peers.to_vec()
    }

    pub(super) fn register_peer(&mut self, peer: NodeId) {
        self.peers.push(peer);
    }

    pub(super) fn register_block(
        &mut self,
        block: Block,
        peer: NodeId,
    ) -> Result<(), AcceptorError> {
        if self.block_hash() != *block.hash() {
            return Err(AcceptorError::BlockHashMismatch {
                expected: self.block_hash(),
                actual: *block.hash(),
                peer,
            });
        }

        if let Err(error) = block.validate(&EmptyValidationMetadata) {
            warn!(%error, "received invalid block");
            // TODO[RC]: Consider renaming `InvalidGossip` and/or restructuring the errors
            return Err(AcceptorError::InvalidGossip(Box::new(
                InvalidGossipError::Block {
                    block_hash: *block.hash(),
                    peer,
                    validation_error: error,
                },
            )));
        }

        self.register_peer(peer);

        if self.block.is_none() {
            self.touch();
            self.block = Some(block);
        }

        Ok(())
    }

    pub(super) fn register_finality_signature(
        &mut self,
        finality_signature: FinalitySignature,
        peer: NodeId,
    ) -> Result<Option<FinalitySignature>, AcceptorError> {
        if let Err(error) = finality_signature.is_verified() {
            warn!(%error, "received invalid finality signature");
            return Err(AcceptorError::InvalidGossip(Box::new(
                InvalidGossipError::FinalitySignature {
                    block_hash: finality_signature.block_hash,
                    peer,
                    validation_error: error,
                },
            )));
        }
        if let Some(era_id) = &self.era_id() {
            if finality_signature.era_id != *era_id {
                return Err(AcceptorError::EraMismatch(
                    EraMismatchError::FinalitySignature {
                        block_hash: finality_signature.block_hash,
                        expected: *era_id,
                        actual: finality_signature.era_id,
                    },
                ));
            }
        }
        self.register_peer(peer);
        let already_had_sufficient_finality = self.has_sufficient_finality;
        let is_new = self
            .signatures
            .insert(
                finality_signature.public_key.clone(),
                finality_signature.clone(),
            )
            .is_none();

        if already_had_sufficient_finality && is_new {
            self.touch();
            return Ok(Some(finality_signature));
        };

        Ok(None)
    }

    pub(super) fn should_store_block(
        &mut self,
        era_validator_weights: &EraValidatorWeights,
    ) -> ShouldStore {
        if self.has_sufficient_finality {
            return ShouldStore::Nothing;
        }

        if self.block.is_none() || self.signatures.is_empty() {
            return ShouldStore::Nothing;
        }

        self.remove_bogus_validators(era_validator_weights);
        if SignatureWeight::Sufficient
            == era_validator_weights.has_sufficient_weight(self.signatures.keys())
        {
            if let Some(block) = self.block.clone() {
                self.touch();
                self.has_sufficient_finality = true;
                return ShouldStore::SufficientlySignedBlock {
                    block,
                    signatures: self.signatures.iter().map(|(_, v)| v.clone()).collect_vec(),
                };
            }
        }

        ShouldStore::Nothing
    }

    pub(super) fn has_sufficient_finality(&self) -> bool {
        self.has_sufficient_finality
    }

    pub(super) fn era_id(&self) -> Option<EraId> {
        if let Some(block) = &self.block {
            return Some(block.header().era_id());
        }
        if let Some(finality_signature) = self.signatures.values().next() {
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

    fn remove_bogus_validators(&mut self, era_validator_weights: &EraValidatorWeights) {
        let bogus_validators = era_validator_weights.bogus_validators(self.signatures.keys());

        bogus_validators.iter().for_each(|bogus_validator| {
            debug!(%bogus_validator, "bogus validator");
            self.signatures.remove(bogus_validator);
        });

        if let Some(block) = &self.block {
            let bogus_validators = self
                .signatures
                .iter()
                .filter(|(_, v)| {
                    v.block_hash != self.block_hash() || v.era_id != block.header().era_id()
                })
                .map(|(k, _)| k.clone())
                .collect_vec();

            bogus_validators.iter().for_each(|bogus_validator| {
                debug!(%bogus_validator, "bogus validator");
                self.signatures.remove(bogus_validator);
            });
        }
    }

    fn touch(&mut self) {
        self.last_progress = Timestamp::now();
    }
}
