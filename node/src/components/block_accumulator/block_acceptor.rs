use std::collections::BTreeMap;

use datasize::DataSize;
use itertools::Itertools;
use tracing::{debug, warn};

use casper_types::{EraId, PublicKey};

use crate::{
    components::block_accumulator::error::{
        EraMismatchError, Error as AcceptorError, InvalidGossipError,
    },
    types::{
        ApprovalsHashes, Block, BlockHash, EmptyValidationMetadata, EraValidatorWeights,
        FetcherItem, FinalitySignature, NodeId, SignatureWeight,
    },
};

/// The outcome of a call to `can_execute`, telling us whether a block has enough signatures and
/// data to be executed, and whether it recently got into that state.
#[derive(Debug, Copy, Clone, PartialEq)]
pub(super) enum CanExecuteOutcome {
    /// Cannot be executed yet.
    No,
    /// Can be executed, and this is the first call to `can_execute` where that is the case.
    NewYes,
    /// Can be executed, but this was already known.
    Yes,
}

#[derive(DataSize, Debug)]
pub(super) struct BlockAcceptor {
    block_hash: BlockHash,
    block: Option<Block>,
    signatures: BTreeMap<PublicKey, FinalitySignature>,
    era_validator_weights: Option<EraValidatorWeights>,
    peers: Vec<NodeId>,
    can_execute: bool,
}

impl BlockAcceptor {
    pub(super) fn new(block_hash: BlockHash, peers: Vec<NodeId>) -> Self {
        Self {
            block_hash,
            era_validator_weights: None,
            block: None,
            signatures: BTreeMap::new(),
            peers,
            can_execute: false,
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
            can_execute: false,
        }
    }

    pub(super) fn peers(&self) -> Vec<NodeId> {
        self.peers.to_vec()
    }

    pub(super) fn register_peer(&mut self, peer: NodeId) {
        self.peers.push(peer);
    }

    pub(super) fn refresh(self, era_validator_weights: EraValidatorWeights) -> Self {
        let block_hash = self.block_hash;
        let signatures = if self.signatures.is_empty() {
            self.signatures
        } else {
            let mut ret = BTreeMap::new();
            let mut public_keys = era_validator_weights.validator_public_keys();
            for (k, v) in self.signatures {
                if public_keys.contains(&k)
                    && v.block_hash == block_hash
                    && v.era_id == era_validator_weights.era_id()
                {
                    ret.insert(k, v);
                }
            }
            ret
        };

        let peers = self.peers;

        Self {
            block_hash,
            era_validator_weights: Some(era_validator_weights),
            block: self.block,
            signatures,
            peers,
            can_execute: false,
        }
    }

    pub(super) fn register_era_validator_weights(
        &mut self,
        era_validator_weights: EraValidatorWeights,
    ) -> Result<(), AcceptorError> {
        if self.era_validator_weights.is_some() {
            return Err(AcceptorError::DuplicatedEraValidatorWeights {
                era_id: era_validator_weights.era_id(),
            });
        }

        if let Some(era_id) = self.era_id() {
            let evw_era_id = era_validator_weights.era_id();
            if evw_era_id != era_id {
                return Err(AcceptorError::EraMismatch(
                    EraMismatchError::EraValidatorWeights {
                        block_hash: self.block_hash,
                        expected: era_id,
                        actual: evw_era_id,
                    },
                ));
            }
        }
        self.era_validator_weights = Some(era_validator_weights);
        self.remove_bogus_validators();
        Ok(())
    }

    pub(super) fn register_block(
        &mut self,
        block: &Block,
        peer: NodeId,
    ) -> Result<(), AcceptorError> {
        if self.block_hash() != *block.hash() {
            return Err(AcceptorError::BlockHashMismatch {
                expected: self.block_hash(),
                actual: *block.hash(),
                peer,
            });
        }

        // todo!() - return the senders of the invalid signatures.
        self.signatures
            .retain(|_, signature| signature.era_id == block.header().era_id());

        if let Some(era_validator_weights) = self.era_validator_weights.as_ref() {
            if era_validator_weights.era_id() != block.header().era_id() {
                self.era_validator_weights = None;
            }
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
            self.block = Some(block.clone());
            self.remove_bogus_validators();
        }
        Ok(())
    }

    pub(super) fn register_finality_signature(
        &mut self,
        finality_signature: FinalitySignature,
        peer: NodeId,
    ) -> Result<(), AcceptorError> {
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
        self.signatures
            .insert(finality_signature.public_key.clone(), finality_signature);
        self.remove_bogus_validators();
        Ok(())
    }

    pub(super) fn can_execute(&mut self) -> CanExecuteOutcome {
        if self.can_execute {
            return CanExecuteOutcome::Yes;
        }

        let missing_elements = self.block.is_none()
            || self.era_validator_weights.is_none()
            || self.signatures.is_empty();

        if missing_elements {
            return CanExecuteOutcome::No;
        }

        if let Some(evw) = &self.era_validator_weights {
            if SignatureWeight::Sufficient == evw.has_sufficient_weight(self.signatures.keys()) {
                self.can_execute = true;
                return CanExecuteOutcome::NewYes;
            }
        }
        CanExecuteOutcome::No
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

    pub(super) fn block_era_and_height(&self) -> Option<(EraId, u64)> {
        if let Some(era_id) = self.era_id() {
            if let Some(height) = self.block_height() {
                return Some((era_id, height));
            }
        }
        None
    }

    pub(super) fn executable_block_and_signatures(
        &mut self,
    ) -> Option<(Block, Vec<FinalitySignature>)> {
        if self.can_execute() == CanExecuteOutcome::No {
            return None;
        }

        if let Some(block) = self.block.clone() {
            return Some((block, self.signatures.values().cloned().collect_vec()));
        }
        None
    }

    pub(super) fn block(&self) -> Option<&Block> {
        self.block.as_ref()
    }

    pub(super) fn block_hash(&self) -> BlockHash {
        self.block_hash
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
}
