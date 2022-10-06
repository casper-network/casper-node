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
        ApprovalsHashes, Block, BlockHash, EraValidatorWeights, FetcherItem, FinalitySignature,
        Item, NodeId, SignatureWeight,
    },
};

#[derive(DataSize, Debug)]
pub(super) struct BlockAcceptor {
    block_hash: BlockHash,
    block: Option<Block>,
    approvals_hashes: Option<ApprovalsHashes>,
    signatures: BTreeMap<PublicKey, FinalitySignature>,
    era_validator_weights: Option<EraValidatorWeights>,
    peers: Vec<NodeId>,
}

impl BlockAcceptor {
    pub(super) fn new(block_hash: BlockHash, peers: Vec<NodeId>) -> Self {
        Self {
            block_hash,
            era_validator_weights: None,
            block: None,
            approvals_hashes: None,
            signatures: BTreeMap::new(),
            peers,
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
            approvals_hashes: None,
            signatures: BTreeMap::new(),
            peers,
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

        let approvals_hashes = match self.approvals_hashes {
            None => None,
            Some(approvals_hashes) => {
                if *approvals_hashes.era_id() != era_validator_weights.era_id() {
                    None
                } else {
                    Some(approvals_hashes)
                }
            }
        };

        let block = match self.block {
            None => None,
            Some(block) => {
                if block.header().era_id() != era_validator_weights.era_id() {
                    None
                } else {
                    Some(block)
                }
            }
        };

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
            block,
            approvals_hashes,
            signatures,
            peers,
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

        let era_id_and_block_hash = self.maybe_era_id_and_block_hash();

        if let Some((era_id, block_hash)) = era_id_and_block_hash {
            let evw_era_id = era_validator_weights.era_id();
            if evw_era_id != *era_id {
                return Err(AcceptorError::EraMismatch(
                    EraMismatchError::EraValidatorWeights {
                        block_hash: *block_hash,
                        expected: *era_id,
                        actual: evw_era_id,
                    },
                ));
            }
        }
        self.era_validator_weights = Some(era_validator_weights);
        self.remove_bogus_validators();
        Ok(())
    }

    fn maybe_era_id_and_block_hash(&mut self) -> Option<(&EraId, &BlockHash)> {
        match (self.approvals_hashes, self.block) {
            (Some(approvals_hashes), Some(_)) | (Some(approvals_hashes), None) => {
                Some((approvals_hashes.era_id(), approvals_hashes.block_hash()))
            }
            (None, Some(block)) => Some((&block.header().era_id(), block.hash())),
            (None, None) => None,
        }
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

        if let Err(error) = block.validate(&()) {
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

        if let Some(approvals_hashes) = self.approvals_hashes {
            let state_root_hash = block.state_root_hash();
            let era_id = block.header().era_id();

            if let Err(error) = approvals_hashes.validate(block) {
                warn!(%error, "received invalid approvals hashes");
                self.approvals_hashes = None;
                return Err(AcceptorError::InvalidGossip(Box::new(
                    InvalidGossipError::ApprovalsHashes {
                        block_hash: *approvals_hashes.block_hash(),
                        peer,
                        validation_error: error,
                    },
                )));
            }
        }

        self.register_peer(peer);

        if self.block.is_none() {
            self.block = Some(*block);
            self.remove_bogus_validators();
        }
        Ok(())
    }

    pub(super) fn register_approvals_hashes(
        &mut self,
        approvals_hashes: &ApprovalsHashes,
        peer: NodeId,
    ) -> Result<(), AcceptorError> {
        if self.block_hash() != *approvals_hashes.block_hash() {
            return Err(AcceptorError::BlockHashMismatch {
                expected: self.block_hash(),
                actual: *approvals_hashes.block_hash(),
                peer,
            });
        }

        if let Some(block) = self.block {
            let state_root_hash = block.state_root_hash();
            let era_id = block.header().era_id();
            if let Err(error) = approvals_hashes.validate(&block) {
                warn!(%error, "received invalid approvals hashes");
                return Err(AcceptorError::InvalidGossip(Box::new(
                    InvalidGossipError::ApprovalsHashes {
                        block_hash: *approvals_hashes.block_hash(),
                        peer,
                        validation_error: error,
                    },
                )));
            }
        }

        self.register_peer(peer);

        if self.approvals_hashes.is_none() {
            self.approvals_hashes = Some(*approvals_hashes);
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
        if let Some(era_validator_weights) = self.era_validator_weights {
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

    pub(super) fn can_execute(&self) -> bool {
        let missing_elements = self.block.is_none()
            || self.approvals_hashes.is_none()
            || self.era_validator_weights.is_none()
            || self.signatures.is_empty();

        if missing_elements {
            return false;
        }

        if let Some(evw) = &self.era_validator_weights {
            if SignatureWeight::Sufficient == evw.has_sufficient_weight(self.signatures.keys()) {
                return true;
            }
        }
        false
    }

    pub(super) fn era_id(&self) -> Option<EraId> {
        let era_id_and_block_hash = self.maybe_era_id_and_block_hash();
        if let Some((era_id, _)) = era_id_and_block_hash {
            return Some(*era_id);
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
        &self,
    ) -> Option<(Block, ApprovalsHashes, Vec<FinalitySignature>)> {
        if !self.can_execute() {
            return None;
        }

        if let (Some(block), Some(approvals_hashes)) = (self.block, self.approvals_hashes) {
            return Some((
                block,
                approvals_hashes,
                self.signatures.values().cloned().collect_vec(),
            ));
        }
        None
    }

    pub(super) fn block_and_approvals_hashes(&self) -> Option<(&Block, &ApprovalsHashes)> {
        if let (Some(ref block), Some(ref approvals_hashes)) = (&self.block, &self.approvals_hashes)
        {
            return Some((block, approvals_hashes));
        }
        None
    }

    pub(super) fn block_hash(&self) -> BlockHash {
        self.block_hash
    }

    pub(super) fn approvals_hashes(&self) -> Option<&ApprovalsHashes> {
        self.approvals_hashes.as_ref()
    }

    pub(super) fn has_era_validator_weights(&self) -> bool {
        self.era_validator_weights.is_some()
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
