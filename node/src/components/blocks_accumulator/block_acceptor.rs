use std::collections::BTreeMap;

use datasize::DataSize;
use num_rational::Ratio;
use tracing::{debug, error, warn};

use casper_types::{EraId, PublicKey, U512};

use super::Error;
use crate::types::ValidatorMatrix;
use crate::{
    components::linear_chain::{self, BlockSignatureError},
    types::{
        BlockAdded, BlockHash, BlockSignatures, EraValidatorWeights, FetcherItem,
        FinalitySignature, SignatureWeight,
    },
    utils::Latch,
};

#[derive(DataSize, Debug)]
pub(super) struct BlockAcceptor {
    block_hash: BlockHash,
    era_id: EraId,
    era_validator_weights: Option<EraValidatorWeights>,
    block_added: Option<BlockAdded>,
    signatures: BTreeMap<PublicKey, FinalitySignature>,
    /// Will remain false until the `block_added` is `Some` and there are strictly sufficient
    /// `signatures`.  Once set to `true`, will remain `true` forever.
    can_execute: Latch<bool>,
}

impl BlockAcceptor {
    pub(super) fn new_from_block_added(
        block_added: BlockAdded,
        era_validator_weights: Option<EraValidatorWeights>,
    ) -> Result<Self, Error> {
        if let Err(error) = block_added.validate(&()) {
            warn!(%error, "received invalid block-added");
            return Err(Error::InvalidBlockAdded(error));
        }
        let block_era = block_added.block.header().era_id();
        if let Some(weights) = era_validator_weights.as_ref() {
            if weights.era_id() != block_era {
                error!(
                    %block_era,
                    validator_weights_era = %weights.era_id(),
                    "validator weights of different era than block provided"
                );
                return Err(Error::WrongEraWeights {
                    block_era,
                    validator_weights_era: weights.era_id(),
                });
            }
        }
        Ok(Self {
            block_hash: *block_added.block.hash(),
            era_id: block_added.block.header().era_id(),
            era_validator_weights,
            block_added: Some(block_added),
            signatures: BTreeMap::default(),
            can_execute: Latch::new(false),
        })
    }

    pub(super) fn new_from_finality_signature(
        finality_signature: FinalitySignature,
        era_validator_weights: Option<EraValidatorWeights>,
    ) -> Result<Self, Error> {
        if let Err(error) = finality_signature.is_verified() {
            warn!(%error, "received invalid finality signature");
            return Err(Error::InvalidFinalitySignature(error));
        }
        if let Some(weights) = era_validator_weights.as_ref() {
            if weights.era_id() != finality_signature.era_id {
                error!(
                    block_era = %finality_signature.era_id,
                    validator_weights_era = %weights.era_id(),
                    "validator weights of different era than finality signature provided"
                );
                return Err(Error::WrongEraWeights {
                    block_era: finality_signature.era_id,
                    validator_weights_era: weights.era_id(),
                });
            }
        }

        let mut signatures = BTreeMap::new();
        let era_id = finality_signature.era_id;
        let block_hash = finality_signature.block_hash;
        signatures.insert(finality_signature.public_key.clone(), finality_signature);
        Ok(Self {
            block_hash,
            era_id,
            era_validator_weights,
            block_added: None,
            signatures,
            can_execute: Latch::new(false),
        })
    }

    pub(super) fn remove_bogus_validators(
        &mut self,
        validator_matrix: &ValidatorMatrix,
    ) -> Option<Vec<PublicKey>> {
        let bogus_validators =
            validator_matrix.bogus_validators(self.era_id(), self.signatures.keys())?;

        bogus_validators.iter().for_each(|bogus_validator| {
            debug!(%bogus_validator, "bogus validator");
            self.signatures.remove(bogus_validator);
        });

        Some(bogus_validators)
    }

    /// Returns true if adding the signature was successful and if by doing so, the block now
    /// becomes executable (i.e. `self.can_execute()` now returns true).
    pub(super) fn register_signature(
        &mut self,
        finality_signature: FinalitySignature,
    ) -> Result<bool, Error> {
        // TODO: verify sig
        // TODO: What to do when we receive multiple valid finality_signature from single
        // public_key? TODO: What to do when we receive too many finality_signature from
        // single peer?
        if let Some(block) = self
            .block_added
            .as_ref()
            .map(|block_added| &block_added.block)
        {
            if block.header().era_id() != finality_signature.era_id {
                warn!(block_hash = %block.hash(), "received finality signature with invalid era");
                // We should not add this signature.
                // TODO: Return an Error here
                return Err(Error::FinalitySignatureWithWrongEra {
                    finality_signature,
                    correct_era: block.header().era_id(),
                });
            }
        }

        // TODO - should do cumulative counting in block_acceptor to avoid calling expensive
        //        `has_sufficient_weight` many times.
        let could_execute = self.can_execute();
        self.signatures
            .insert(finality_signature.public_key.clone(), finality_signature);
        let can_execute = self.can_execute();
        Ok(can_execute && !could_execute)
    }

    /// Returns true if adding the block was successful and if by doing so, the block now
    /// becomes executable (i.e. `self.can_execute()` now returns true).
    pub(super) fn register_block(&mut self, block_added: BlockAdded) -> Result<bool, Error> {
        if self.block_added.is_some() {
            debug!(block_hash = %block_added.block.hash(), "received duplicate block-added");
            return Ok(false);
        }

        if let Err(error) = block_added.validate(&()) {
            warn!(%error, "received invalid block");
            return Err(Error::InvalidBlockAdded(error));
        }

        // TODO: Maybe disconnect from senders of the incorrect signatures.
        self.signatures.retain(|_, finality_signature| {
            finality_signature.era_id == block_added.block.header().era_id()
        });

        let could_execute = self.can_execute();
        self.block_added = Some(block_added);
        let can_execute = self.can_execute();
        Ok(can_execute && !could_execute)
    }

    /// Returns true if adding the signature was successful and if by doing so, the block now
    /// becomes executable (i.e. `self.can_execute()` now returns true).
    pub(super) fn register_era_validator_weights(
        &mut self,
        era_validator_weights: EraValidatorWeights,
    ) -> Result<bool, Error> {
        if era_validator_weights.era_id() != self.era_id {
            error!(
                block_era = %self.era_id,
                validator_weights_era = %era_validator_weights.era_id(),
                "received validator weights of wrong era"
            );
            return Err(Error::WrongEraWeights {
                block_era: self.era_id,
                validator_weights_era: era_validator_weights.era_id(),
            });
        }
        let could_execute = self.can_execute();
        self.era_validator_weights = Some(era_validator_weights);
        let can_execute = self.can_execute();
        Ok(can_execute && !could_execute)
    }

    pub(super) fn has_block_added(&self) -> bool {
        self.block_added.is_some()
    }

    pub(super) fn can_execute(&mut self) -> bool {
        if *self.can_execute {
            return true;
        }

        if self.block_added.is_none() {
            return false;
        }

        if let Some(era_validator_weights) = self.era_validator_weights.as_ref() {
            if SignatureWeight::Sufficient
                == era_validator_weights.has_sufficient_weight(self.signatures.keys())
            {
                let _updated = self.can_execute.set(true);
                debug_assert!(_updated, "should only ever set once");
            }
        }

        *self.can_execute
    }

    pub(super) fn block_height(&self) -> Option<u64> {
        self.block_added
            .as_ref()
            .map(|block_added| block_added.block.header().height())
    }

    pub(super) fn era_id(&self) -> EraId {
        self.era_id
    }
}
