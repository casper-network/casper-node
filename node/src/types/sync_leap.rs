use std::{
    fmt::{self, Display, Formatter},
    iter,
};

use itertools::Itertools;
use num_rational::Ratio;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use casper_types::{crypto, EraId};

use crate::{
    components::linear_chain::{self, BlockSignatureError},
    types::{
        error::BlockHeaderWithMetadataValidationError, BlockHash, BlockHeader,
        BlockHeaderWithMetadata, FetcherItem, Item, Tag,
    },
};

/// Headers and signatures required to prove that if a given trusted block hash is on the correct
/// chain, then so is a later header, which should be the most recent one according to the sender.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
pub(crate) struct SyncLeap {
    /// The header of the trusted block specified by hash by the requester.
    pub trusted_block_header: BlockHeader,
    /// The block headers of the trusted block's ancestors, back to the most recent switch block.
    /// If the trusted one is already a switch block, this is empty.
    /// Sorted from highest to lowest.
    pub trusted_ancestor_headers: Vec<BlockHeader>,
    /// The headers of all switch blocks known to the sender, after the trusted block but before
    /// their highest block, with signatures, plus the signed highest block.
    /// Sorted from lowest to highest.
    pub signed_block_headers: Vec<BlockHeaderWithMetadata>,
}

impl SyncLeap {
    pub(crate) fn highest_era(&self) -> EraId {
        self.signed_block_headers
            .iter()
            .map(|header_with_metadata| header_with_metadata.block_header.era_id())
            .max()
            .unwrap_or_else(|| self.trusted_block_header.era_id())
    }
}

impl Display for SyncLeap {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "sync leap message for trusted hash {}",
            self.trusted_block_header.hash()
        )
    }
}

impl Item for SyncLeap {
    type Id = BlockHash;

    const TAG: Tag = Tag::SyncLeap;

    fn id(&self) -> Self::Id {
        self.trusted_block_header.hash()
    }
}

impl FetcherItem for SyncLeap {
    type ValidationError = SyncLeapValidationError;
    type ValidationMetadata = Ratio<u64>;

    fn validate(
        &self,
        finality_threshold_fraction: &Ratio<u64>,
    ) -> Result<(), Self::ValidationError> {
        // TODO: Possibly check the size of the collections.

        // The oldest provided block must be a switch block, so that we know the validators
        // who are expected to sign later blocks.
        let oldest_header = self
            .trusted_ancestor_headers
            .first()
            .unwrap_or(&self.trusted_block_header);
        // The header chain should only go back until it hits _one_ switch block.
        for header in self
            .trusted_ancestor_headers
            .iter()
            .chain(iter::once(&self.trusted_block_header))
            .skip(1)
        {
            if header.is_switch_block() {
                return Err(SyncLeapValidationError::UnexpectedSwitchBlock);
            }
        }
        // All headers must have the same protocol versions: sync leaps across upgrade boundaries
        // are not supported.
        let protocol_version = self.trusted_block_header.protocol_version();
        if self
            .trusted_ancestor_headers
            .iter()
            .any(|header| header.protocol_version() != protocol_version)
            || self.signed_block_headers.iter().any(|signed_header| {
                signed_header.block_header.protocol_version() != protocol_version
            })
        {
            return Err(SyncLeapValidationError::MultipleProtocolVersions);
        }
        // The chain from the oldest switch block to the trusted one must be contiguous.
        for (parent_header, child_header) in self
            .trusted_ancestor_headers
            .iter()
            .chain(iter::once(&self.trusted_block_header))
            .tuple_windows()
        {
            if *child_header.parent_hash() != parent_header.hash() {
                return Err(SyncLeapValidationError::HeadersNotContiguous);
            }
        }

        let mut validator_weights = oldest_header
            .next_era_validator_weights()
            .ok_or(SyncLeapValidationError::MissingSwitchBlock)?;
        let mut validators_era_id = oldest_header.next_block_era_id();

        // All but (possibly) the last signed header must be switch blocks.
        for signed_header in self.signed_block_headers.iter().rev().skip(1) {
            if !signed_header.block_header.is_switch_block() {
                return Err(SyncLeapValidationError::SignedHeadersNotInConsecutiveEras);
            }
        }

        // Finally we verify the signatures, and check that their weight is sufficient.
        for signed_header in &self.signed_block_headers {
            if validators_era_id != signed_header.block_header.era_id() {
                return Err(SyncLeapValidationError::SignedHeadersNotInConsecutiveEras);
            }
            match linear_chain::check_sufficient_block_signatures(
                &validator_weights,
                *finality_threshold_fraction,
                Some(&signed_header.block_signatures),
            ) {
                Ok(()) => (),
                Err(err) => return Err(SyncLeapValidationError::HeadersNotSufficientlySigned(err)),
            }
            signed_header
                .validate(&())
                .map_err(SyncLeapValidationError::BlockWithMetadata)?;
            signed_header
                .block_signatures
                .verify()
                .map_err(SyncLeapValidationError::Crypto)?;

            if let Some(next_validator_weights) =
                signed_header.block_header.next_era_validator_weights()
            {
                validator_weights = next_validator_weights;
                validators_era_id = signed_header.block_header.era_id();
            }
        }

        Ok(())
    }
}

#[derive(Error, Debug)]
pub(crate) enum SyncLeapValidationError {
    #[error("The oldest provided block is not a switch block.")]
    MissingSwitchBlock,
    #[error("The headers chain before the trusted hash contains more than one switch block.")]
    UnexpectedSwitchBlock,
    #[error("The provided headers have different protocol versions.")]
    MultipleProtocolVersions,
    #[error("The sequence of headers up to the trusted one is not contiguous.")]
    HeadersNotContiguous,
    #[error("The signed headers are not in consecutive eras.")]
    SignedHeadersNotInConsecutiveEras,
    #[error(transparent)]
    HeadersNotSufficientlySigned(BlockSignatureError),
    #[error("The block signatures are not cryptographically valid: {0}")]
    Crypto(crypto::Error),
    #[error(transparent)]
    BlockWithMetadata(BlockHeaderWithMetadataValidationError),
}
