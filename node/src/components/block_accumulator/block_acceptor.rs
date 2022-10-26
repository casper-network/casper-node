use std::collections::BTreeMap;

use datasize::DataSize;
use itertools::Itertools;
use tracing::{debug, error, warn};

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
    era_validator_weights: Option<EraValidatorWeights>,
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
            era_validator_weights: None,
            block: None,
            signatures: BTreeMap::new(),
            peers,
            has_sufficient_finality: false,
            last_progress: Timestamp::now(),
        }
    }

    pub(super) fn new_with_validator_weights(
        block_hash: BlockHash,
        era_validator_weights: EraValidatorWeights,
        peers: Vec<NodeId>,
    ) -> Self {
        Self {
            block_hash,
            era_validator_weights: Some(era_validator_weights),
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

    pub(super) fn refresh(
        &mut self,
        era_validator_weights: EraValidatorWeights,
    ) -> Result<ShouldStore, AcceptorError> {
        if self.era_validator_weights.is_some() {
            return Ok(ShouldStore::Nothing);
        }

        if let Some(expected) = self.block.as_ref().map(|block| block.header().era_id()) {
            if expected != era_validator_weights.era_id() {
                return Err(AcceptorError::EraMismatch(
                    EraMismatchError::EraValidatorWeights {
                        block_hash: self.block_hash,
                        expected,
                        actual: era_validator_weights.era_id(),
                    },
                ));
            }
        }

        debug_assert!(!self.has_sufficient_finality);

        let block_hash = self.block_hash;
        let mut validators = era_validator_weights.validator_public_keys();
        self.signatures.retain(|pub_key, sig| {
            validators.contains(pub_key)
                && sig.block_hash == block_hash
                && sig.era_id == era_validator_weights.era_id()
        });

        if self.has_sufficient_finality() {
            match &self.block {
                Some(block) => {
                    let signatures = self.signatures.values().cloned().collect();
                    let block = block.clone();
                    self.touch();
                    return Ok(ShouldStore::SufficientlySignedBlock { block, signatures });
                }
                None => {
                    error!("self.block should be Some due to check in `has_sufficient_finality`");
                }
            }
        }
        Ok(ShouldStore::Nothing)
    }

    pub(super) fn register_block(
        &mut self,
        block: Block,
        peer: NodeId,
    ) -> Result<ShouldStore, AcceptorError> {
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

        let mut removed_validator_weights = false;
        let era_id = block.header().era_id();
        if self.block.is_none() {
            self.touch();
            if let Some(era_validator_weights) = self.era_validator_weights.as_ref() {
                if era_validator_weights.era_id() != era_id {
                    self.era_validator_weights = None;
                    removed_validator_weights = true;
                }
            }
            self.block = Some(block);
            // todo! - return the senders of the invalid signatures.
            self.remove_bogus_validators();
        }

        if removed_validator_weights {
            return Err(AcceptorError::RemovedValidatorWeights { era_id });
        }

        if self.has_sufficient_finality() {
            self.touch();
            let block = self.block.clone().ok_or_else(|| {
                error!("self.block should be Some due to check in `has_sufficient_finality`");
                AcceptorError::InvalidState
            })?;
            let signatures = self.signatures.values().cloned().collect();
            return Ok(ShouldStore::SufficientlySignedBlock { block, signatures });
        }

        Ok(ShouldStore::Nothing)
    }

    pub(super) fn register_finality_signature(
        &mut self,
        finality_signature: FinalitySignature,
        peer: NodeId,
    ) -> Result<ShouldStore, AcceptorError> {
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
        if let Some(era_validator_weights) = &self.era_validator_weights {
            if finality_signature.era_id != era_validator_weights.era_id() {
                return Err(AcceptorError::EraMismatch(
                    EraMismatchError::FinalitySignature {
                        block_hash: finality_signature.block_hash,
                        expected: era_validator_weights.era_id(),
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
        self.remove_bogus_validators();

        if already_had_sufficient_finality && is_new {
            self.touch();
            return Ok(ShouldStore::SingleSignature(finality_signature));
        };

        if !already_had_sufficient_finality && self.has_sufficient_finality() {
            self.touch();
            let block = self.block.clone().ok_or_else(|| {
                error!("self.block should be Some due to check in `has_sufficient_finality`");
                AcceptorError::InvalidState
            })?;
            let signatures = self.signatures.values().cloned().collect();
            return Ok(ShouldStore::SufficientlySignedBlock { block, signatures });
        }

        Ok(ShouldStore::Nothing)
    }

    pub(super) fn has_sufficient_finality(&mut self) -> bool {
        if self.has_sufficient_finality {
            return true;
        }

        let missing_elements = self.block.is_none()
            || self.era_validator_weights.is_none()
            || self.signatures.is_empty();

        if missing_elements {
            return self.has_sufficient_finality;
        }

        if let Some(evw) = &self.era_validator_weights {
            if SignatureWeight::Sufficient == evw.has_sufficient_weight(self.signatures.keys()) {
                self.has_sufficient_finality = true;
            }
        }

        self.has_sufficient_finality
    }

    pub(super) fn has_validator_weights(&self) -> bool {
        self.era_validator_weights.is_some()
    }

    pub(super) fn era_id(&self) -> Option<EraId> {
        if let Some(block) = &self.block {
            return Some(block.header().era_id());
        }
        if let Some(finality_signature) = self.signatures.values().next() {
            return Some(finality_signature.era_id);
        }
        if let Some(evw) = &self.era_validator_weights {
            return Some(evw.era_id());
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

    fn remove_bogus_validators(&mut self) {
        if let Some(evw) = &self.era_validator_weights {
            let bogus_validators = evw.bogus_validators(self.signatures.keys());

            bogus_validators.iter().for_each(|bogus_validator| {
                debug!(%bogus_validator, "bogus validator");
                self.signatures.remove(bogus_validator);
            });
        }
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
