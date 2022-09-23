use std::collections::BTreeMap;

use datasize::DataSize;
use num_rational::Ratio;
use tracing::{debug, warn};

use casper_types::{EraId, PublicKey, U512};

use super::Error;
use crate::{
    components::linear_chain::{self, BlockSignatureError},
    types::{
        BlockAdded, BlockHash, BlockSignatures, FetcherItem, FinalitySignature, SignatureWeight,
        ValidatorMatrix,
    },
};

#[derive(DataSize, Debug)]
pub(super) struct BlockAcceptor {
    block_hash: BlockHash,
    block_added: Option<BlockAdded>,
    era_id: EraId,
    signatures: BTreeMap<PublicKey, FinalitySignature>,
}

impl BlockAcceptor {
    pub(super) fn new_from_block_added(block_added: BlockAdded) -> Result<Self, Error> {
        if let Err(error) = block_added.validate(&()) {
            warn!(%error, "received invalid block-added");
            return Err(Error::InvalidBlockAdded(error));
        }
        Ok(Self {
            block_hash: *block_added.block.hash(),
            era_id: block_added.block.header().era_id(),
            block_added: Some(block_added),
            signatures: BTreeMap::default(),
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
            block_added: None,
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
        self.signatures
            .insert(finality_signature.public_key.clone(), finality_signature);
        Ok(())
    }

    pub(super) fn register_block(&mut self, block_added: BlockAdded) -> Result<(), Error> {
        if self.block_added.is_some() {
            debug!(block_hash = %block_added.block.hash(), "received duplicate block-added");
            return Ok(());
        }

        if let Err(error) = block_added.validate(&()) {
            warn!(%error, "received invalid block");
            return Err(Error::InvalidBlockAdded(error));
        }

        // TODO: Maybe disconnect from senders of the incorrect signatures.
        self.signatures.retain(|_, finality_signature| {
            finality_signature.era_id == block_added.block.header().era_id()
        });

        self.block_added = Some(block_added);
        Ok(())
    }

    pub(super) fn has_sufficient_signatures(
        &self,
        validator_matrix: &ValidatorMatrix,
    ) -> Option<SignatureWeight> {
        // TODO: Consider caching the sigs directly in the `BlockSignatures` struct, to avoid
        // creating it from `BTreeMap<PublicKey, FinalitySignature>` on every call.
        let mut block_signatures = BlockSignatures::new(self.block_hash, self.era_id);
        self.signatures
            .iter()
            .for_each(|(public_key, finality_signature)| {
                block_signatures.insert_proof(public_key.clone(), finality_signature.signature);
            });

        validator_matrix.has_sufficient_weight(self.era_id, self.signatures.keys())
    }

    pub(super) fn remove_signatures(&mut self, signers: &[PublicKey]) {
        self.signatures
            .retain(|public_key, _| !signers.contains(public_key))
    }

    pub(super) fn has_block_added(&self) -> bool {
        self.block_added.is_some()
    }

    pub(super) fn can_execute(&self, validator_matrix: &ValidatorMatrix) -> bool {
        if self.block_added.is_none() {
            return false;
        }

        Some(SignatureWeight::Sufficient)
            == validator_matrix.has_sufficient_weight(self.era_id(), self.signatures.keys())
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
