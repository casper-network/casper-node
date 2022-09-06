use std::collections::BTreeMap;

use datasize::DataSize;
use num_rational::Ratio;
use tracing::{debug, warn};

use casper_types::{EraId, PublicKey, U512};

use super::{Error, SignaturesFinality};
use crate::{
    components::linear_chain::{self, BlockSignatureError},
    types::{Block, BlockHash, BlockSignatures, FinalitySignature},
};

#[derive(DataSize, Debug)]
pub(super) struct BlockAcceptor {
    block_hash: BlockHash,
    block: Option<Block>,
    era_id: EraId,
    signatures: BTreeMap<PublicKey, FinalitySignature>,
}

impl BlockAcceptor {
    pub(super) fn new_from_block_added(block: Block) -> Result<Self, Error> {
        if let Err(error) = block.verify() {
            warn!(%error, "received invalid block");
            return Err(Error::InvalidBlock(error));
        }
        Ok(Self {
            block_hash: *block.hash(),
            era_id: block.header().era_id(),
            block: Some(block),
            signatures: Default::default(),
        })
    }

    pub(super) fn new_from_finality_signature(
        finality_signature: FinalitySignature,
    ) -> Result<Self, Error> {
        if let Err(error) = finality_signature.is_verified() {
            warn!(%error, "received invalid finality signature");
            return Err(Error::InvalidFinalitySignature(error));
        }

        let mut signatures = BTreeMap::new();
        let era_id = finality_signature.era_id;
        let block_hash = finality_signature.block_hash;
        signatures.insert(finality_signature.public_key.clone(), finality_signature);
        Ok(Self {
            block_hash,
            block: None,
            era_id,
            signatures,
        })
    }

    pub(super) fn register_signature(
        &mut self,
        finality_signature: FinalitySignature,
    ) -> Result<(), Error> {
        // TODO: verify sig
        // TODO: What to do when we receive multiple valid finality_signature from single
        // public_key? TODO: What to do when we receive too many finality_signature from
        // single peer?
        if let Some(block) = self.block.as_ref() {
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
        self.signatures
            .insert(finality_signature.public_key.clone(), finality_signature);
        Ok(())
    }

    pub(super) fn register_block(&mut self, block: Block) -> Result<(), Error> {
        if self.block.is_some() {
            debug!(block_hash = %block.hash(), "received duplicate block");
            return Ok(());
        }

        if let Err(error) = block.verify() {
            warn!(%error, "received invalid block");
            return Err(Error::InvalidBlock(error));
        }

        // TODO: Maybe disconnect from senders of the incorrect signatures.
        self.signatures
            .retain(|_, finality_signature| finality_signature.era_id == block.header().era_id());

        self.block = Some(block);
        Ok(())
    }

    pub(super) fn has_sufficient_signatures(
        &self,
        fault_tolerance_fraction: Ratio<u64>,
        trusted_validator_weights: BTreeMap<PublicKey, U512>,
    ) -> SignaturesFinality {
        // TODO: Consider caching the sigs directly in the `BlockSignatures` struct, to avoid
        // creating it from `BTreeMap<PublicKey, FinalitySignature>` on every call.
        let mut block_signatures = BlockSignatures::new(self.block_hash, self.era_id);
        self.signatures
            .iter()
            .for_each(|(public_key, finality_signature)| {
                block_signatures.insert_proof(public_key.clone(), finality_signature.signature);
            });

        match linear_chain::check_sufficient_block_signatures(
            &trusted_validator_weights,
            fault_tolerance_fraction,
            Some(&block_signatures),
        ) {
            Ok(_) => SignaturesFinality::Sufficient,
            Err(err) => match err {
                BlockSignatureError::BogusValidators {
                    bogus_validators, ..
                } => SignaturesFinality::BogusValidators(*bogus_validators),
                BlockSignatureError::InsufficientWeightForFinality { .. } => {
                    return SignaturesFinality::NotSufficient
                }
                BlockSignatureError::TooManySignatures { .. } => {
                    // This error is returned only when the signatures are proven to be sufficient.
                    SignaturesFinality::Sufficient
                }
            },
        }
    }

    pub(super) fn remove_signatures(&mut self, signers: &[PublicKey]) {
        self.signatures
            .retain(|public_key, _| !signers.contains(&public_key))
    }
}
