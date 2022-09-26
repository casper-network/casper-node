use std::collections::BTreeMap;

use datasize::DataSize;
use itertools::Itertools;
use log::Level::Error;
use num_rational::Ratio;
use tracing::{debug, error, warn};

use casper_types::{EraId, PublicKey, Timestamp, U512};

use crate::{
    components::{
        blocks_accumulator::error::{EraMismatchError, Error as AcceptorError, InvalidGossipError},
        linear_chain::{self, BlockSignatureError},
    },
    types::{
        Block, BlockAdded, BlockHash, BlockSignatures, EraValidatorWeights, FetcherItem,
        FinalitySignature, Item, NodeId, SignatureWeight, ValidatorMatrix,
    },
    utils::Latch,
};

#[derive(DataSize, Debug)]
pub(super) struct BlockGossipAcceptor {
    block_hash: BlockHash,
    block_added: Option<BlockAdded>,
    signatures: BTreeMap<PublicKey, FinalitySignature>,
    era_validator_weights: Option<EraValidatorWeights>,
}

impl BlockGossipAcceptor {
    pub(super) fn new(block_hash: BlockHash) -> Self {
        Self {
            block_hash,
            era_validator_weights: None,
            block_added: None,
            signatures: BTreeMap::new(),
        }
    }

    pub(super) fn new_with_validator_weights(
        block_hash: BlockHash,
        era_validator_weights: EraValidatorWeights,
    ) -> Self {
        Self {
            block_hash,
            era_validator_weights: Some(era_validator_weights),
            block_added: None,
            signatures: BTreeMap::new(),
        }
    }

    pub(super) fn refresh(mut self, era_validator_weights: EraValidatorWeights) -> Self {
        let block_hash = self.block_hash;

        let block_added = match self.block_added {
            None => None,
            Some(block_added) => {
                if block_added.block.header().era_id() != era_validator_weights.era_id() {
                    None
                } else {
                    Some(block_added)
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

        Self {
            block_hash,
            era_validator_weights: Some(era_validator_weights),
            block_added,
            signatures,
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
        if let Some(block_added) = &self.block_added {
            let era_id = block_added.block.header().era_id();
            let evw_era_id = era_validator_weights.era_id();
            if evw_era_id != era_id {
                return Err(AcceptorError::EraMismatch(
                    EraMismatchError::EraValidatorWeights {
                        block_hash: block_added.block.header().id(),
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

    pub(super) fn register_block_added(
        &mut self,
        block_added: BlockAdded,
        peer: NodeId,
    ) -> Result<(), AcceptorError> {
        if let Some(block_added) = &self.block_added {}
        if let Err(error) = block_added.validate(&()) {
            warn!(%error, "received invalid block-added");
            return Err(AcceptorError::InvalidGossip(Box::new(
                InvalidGossipError::BlockAdded {
                    block_hash: block_added.block.header().id(),
                    peer,
                    validation_error: error,
                },
            )));
        }
        if let Some(era_id) = self.era_validator_weights_era_id() {
            let block_era_id = block_added.block.header().era_id();
            if block_era_id != era_id {
                return Err(AcceptorError::EraMismatch(EraMismatchError::Block {
                    block_hash: block_added.block.id(),
                    expected: era_id,
                    actual: block_era_id,
                }));
            }
        }
        if self.block_added.is_none() {
            self.block_added = Some(block_added);
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
            let sig_era_id = finality_signature.era_id;
            if sig_era_id != *era_id {
                return Err(AcceptorError::EraMismatch(
                    EraMismatchError::FinalitySignature {
                        block_hash: finality_signature.block_hash,
                        expected: *era_id,
                        actual: sig_era_id,
                    },
                ));
            }
        }
        if let Some(era_id) = &self.era_validator_weights_era_id() {
            let sig_era_id = finality_signature.era_id;
            if sig_era_id != *era_id {
                return Err(AcceptorError::EraMismatch(
                    EraMismatchError::FinalitySignature {
                        block_hash: finality_signature.block_hash,
                        expected: *era_id,
                        actual: sig_era_id,
                    },
                ));
            }
        }
        self.signatures
            .insert(finality_signature.public_key.clone(), finality_signature);
        self.remove_bogus_validators();
        Ok(())
    }

    pub(super) fn can_execute(&self) -> bool {
        let missing_elements = self.block_added.is_none()
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
        if let Some(era_id) = self.block_era_id() {
            return Some(era_id);
        }
        if let Some(era_id) = self.era_validator_weights_era_id() {
            return Some(era_id);
        }
        None
    }

    fn era_validator_weights_era_id(&self) -> Option<EraId> {
        if let Some(evw) = &self.era_validator_weights {
            return Some(evw.era_id());
        }
        None
    }

    fn block_era_id(&self) -> Option<EraId> {
        if let Some(block_added) = &self.block_added {
            return Some(block_added.block.header().era_id());
        }
        None
    }

    pub(super) fn block_height(&self) -> Option<u64> {
        self.block_added
            .as_ref()
            .map(|block_added| block_added.block.header().height())
    }

    pub(super) fn block_era_and_height(&self) -> Option<(EraId, u64)> {
        if let Some(era_id) = self.era_id() {
            if let Some(height) = self.block_height() {
                return Some((era_id, height));
            }
        }
        None
    }

    pub(super) fn executable_block(&self) -> Option<Block> {
        if self.can_execute() == false {
            return None;
        }
        if let Some(block_added) = &self.block_added {
            return Some(block_added.block.clone());
        }
        None
    }

    pub(super) fn executable_block_and_signatures(
        &self,
    ) -> Option<(Block, Vec<FinalitySignature>)> {
        if self.can_execute() == false {
            return None;
        }
        if let Some(block_added) = &self.block_added {
            return Some((
                block_added.block.clone(),
                self.signatures.values().cloned().collect_vec(),
            ));
        }
        None
    }

    pub(super) fn block_hash(&self) -> BlockHash {
        self.block_hash
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
        if let Some(block_added) = &self.block_added {
            let bogus_validators = self
                .signatures
                .iter()
                .filter(|(k, v)| {
                    v.block_hash != block_added.block.id()
                        || v.era_id != block_added.block.header().era_id()
                })
                .map(|(k, v)| k.clone())
                .collect_vec();

            bogus_validators.iter().for_each(|bogus_validator| {
                debug!(%bogus_validator, "bogus validator");
                self.signatures.remove(bogus_validator);
            });
        }
    }
}
