use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
    iter,
    sync::Arc,
};

use datasize::DataSize;
use itertools::Itertools;
use num_rational::Ratio;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use casper_types::{crypto, EraId};
use tracing::error;

use crate::{
    types::{
        error::BlockHeaderWithMetadataValidationError, BlockHash, BlockHeader,
        BlockHeaderWithMetadata, BlockSignatures, Chainspec, EraValidatorWeights, FetcherItem,
        Item, Tag,
    },
    utils::{self, BlockSignatureError},
};

#[derive(Error, Debug)]
pub(crate) enum SyncLeapValidationError {
    #[error("The provided headers don't have the current protocol version.")]
    WrongProtocolVersion,
    #[error("No ancestors of the trusted block provided.")]
    MissingTrustedAncestors,
    #[error("The SyncLeap does not contain proof that all its headers are on the right chain.")]
    IncompleteProof,
    #[error(transparent)]
    HeadersNotSufficientlySigned(BlockSignatureError),
    #[error("The block signatures are not cryptographically valid: {0}")]
    Crypto(crypto::Error),
    #[error(transparent)]
    BlockWithMetadata(BlockHeaderWithMetadataValidationError),
    #[error("Too many switch blocks: leaping across that many eras is not allowed.")]
    TooManySwitchBlocks,
    #[error("Trusted ancestor headers must be in reverse chronological order.")]
    TrustedAncestorsNotSorted,
    #[error("Last trusted ancestor is not a switch block.")]
    MissingAncestorSwitchBlock,
    #[error("Only the last trusted ancestor is allowed to be a switch block.")]
    UnexpectedAncestorSwitchBlock,
    #[error("Signed block headers present despite trusted_ancestor_only flag.")]
    UnexpectedSignedBlockHeaders,
}

/// Identifier for a SyncLeap.
#[derive(Debug, Serialize, Deserialize, Copy, Clone, Hash, PartialEq, Eq, DataSize)]
pub(crate) struct SyncLeapIdentifier {
    /// The block hash of the initial trusted block.
    block_hash: BlockHash,
    /// If true, signed_block_headers are not required.
    trusted_ancestor_only: bool,
}

impl SyncLeapIdentifier {
    pub(crate) fn sync_to_tip(block_hash: BlockHash) -> Self {
        SyncLeapIdentifier {
            block_hash,
            trusted_ancestor_only: false,
        }
    }

    pub(crate) fn sync_to_historical(block_hash: BlockHash) -> Self {
        SyncLeapIdentifier {
            block_hash,
            trusted_ancestor_only: true,
        }
    }

    pub(crate) fn block_hash(&self) -> BlockHash {
        self.block_hash
    }

    pub(crate) fn trusted_ancestor_only(&self) -> bool {
        self.trusted_ancestor_only
    }
}

impl Display for SyncLeapIdentifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} trusted_ancestor_only: {}",
            self.block_hash, self.trusted_ancestor_only
        )
    }
}

/// Headers and signatures required to prove that if a given trusted block hash is on the correct
/// chain, then so is a later header, which should be the most recent one according to the sender.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, DataSize)]
pub(crate) struct SyncLeap {
    /// Requester indicates if they want only the header and ancestor headers,
    /// of if they want everything.
    pub trusted_ancestor_only: bool,
    /// The header of the trusted block specified by hash by the requester.
    pub trusted_block_header: BlockHeader,
    /// The block headers of the trusted block's ancestors, back to the most recent switch block.
    pub trusted_ancestor_headers: Vec<BlockHeader>,
    /// The headers of all switch blocks known to the sender, after the trusted block but before
    /// their highest block, with signatures, plus the signed highest block.
    pub signed_block_headers: Vec<BlockHeaderWithMetadata>,
}

impl SyncLeap {
    pub(crate) fn switch_blocks(&self) -> impl Iterator<Item = &BlockHeader> {
        self.headers().filter(|header| header.is_switch_block())
    }

    pub(crate) fn era_validator_weights(
        &self,
        fault_tolerance_fraction: Ratio<u64>,
    ) -> impl Iterator<Item = EraValidatorWeights> + '_ {
        self.switch_blocks()
            .find(|block_header| block_header.is_genesis())
            .into_iter()
            .flat_map(move |block_header| {
                Some(EraValidatorWeights::new(
                    EraId::default(),
                    block_header.next_era_validator_weights().cloned()?,
                    fault_tolerance_fraction,
                ))
            })
            .chain(self.switch_blocks().flat_map(move |block_header| {
                Some(EraValidatorWeights::new(
                    block_header.next_block_era_id(),
                    block_header.next_era_validator_weights().cloned()?,
                    fault_tolerance_fraction,
                ))
            }))
    }

    pub(crate) fn highest_block_height(&self) -> u64 {
        self.headers()
            .map(BlockHeader::height)
            .max()
            .unwrap_or_else(|| self.trusted_block_header.height())
    }

    pub(crate) fn highest_block_header(&self) -> (&BlockHeader, Option<&BlockSignatures>) {
        let header = self
            .headers()
            .max_by_key(|header| header.height())
            .unwrap_or(&self.trusted_block_header);
        let signatures = self
            .signed_block_headers
            .iter()
            .find(|block_header_with_metadata| {
                block_header_with_metadata.block_header.height() == header.height()
            })
            .map(|block_header_with_metadata| &block_header_with_metadata.block_signatures);
        (header, signatures)
    }

    pub(crate) fn highest_block_hash(&self) -> BlockHash {
        self.highest_block_header().0.block_hash()
    }

    pub(crate) fn headers(&self) -> impl Iterator<Item = &BlockHeader> {
        iter::once(&self.trusted_block_header)
            .chain(&self.trusted_ancestor_headers)
            .chain(self.signed_block_headers.iter().map(|sh| &sh.block_header))
    }
}

impl Display for SyncLeap {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "sync leap message for trusted {}",
            self.trusted_block_header.block_hash()
        )
    }
}

impl Item for SyncLeap {
    type Id = SyncLeapIdentifier;

    fn id(&self) -> Self::Id {
        SyncLeapIdentifier {
            block_hash: self.trusted_block_header.block_hash(),
            trusted_ancestor_only: self.trusted_ancestor_only,
        }
    }
}

impl FetcherItem for SyncLeap {
    type ValidationError = SyncLeapValidationError;
    type ValidationMetadata = Arc<Chainspec>;
    const TAG: Tag = Tag::SyncLeap;

    fn validate(&self, chainspec: &Arc<Chainspec>) -> Result<(), Self::ValidationError> {
        if self.trusted_ancestor_headers.is_empty() && self.trusted_block_header.height() > 0 {
            return Err(SyncLeapValidationError::MissingTrustedAncestors);
        }
        if self.signed_block_headers.len() as u64
            > chainspec.core_config.recent_era_count().saturating_add(1)
        {
            return Err(SyncLeapValidationError::TooManySwitchBlocks);
        }
        if self
            .trusted_ancestor_headers
            .iter()
            .tuple_windows()
            .any(|(child, parent)| *child.parent_hash() != parent.block_hash())
        {
            return Err(SyncLeapValidationError::TrustedAncestorsNotSorted);
        }
        let mut trusted_ancestor_iter = self.trusted_ancestor_headers.iter().rev();
        if let Some(last_ancestor) = trusted_ancestor_iter.next() {
            if !last_ancestor.is_switch_block() {
                return Err(SyncLeapValidationError::MissingAncestorSwitchBlock);
            }
        }
        if trusted_ancestor_iter.any(BlockHeader::is_switch_block) {
            return Err(SyncLeapValidationError::UnexpectedAncestorSwitchBlock);
        }
        if self.trusted_ancestor_only && !self.signed_block_headers.is_empty() {
            return Err(SyncLeapValidationError::UnexpectedSignedBlockHeaders);
        }

        let mut headers: BTreeMap<BlockHash, &BlockHeader> = self
            .headers()
            .map(|header| (header.block_hash(), header))
            .collect();
        let mut signatures: BTreeMap<EraId, Vec<&BlockSignatures>> = BTreeMap::new();
        for signed_header in &self.signed_block_headers {
            signatures
                .entry(signed_header.block_signatures.era_id)
                .or_default()
                .push(&signed_header.block_signatures);
        }

        let protocol_version = chainspec.protocol_version();
        if self.signed_block_headers.is_empty() {
            if self.trusted_block_header.protocol_version() != protocol_version
                && !chainspec
                    .protocol_config
                    .is_last_block_before_activation(&self.trusted_block_header)
            {
                return Err(SyncLeapValidationError::WrongProtocolVersion);
            }
        } else if self
            .signed_block_headers
            .iter()
            .any(|header_with_metadata| {
                header_with_metadata.block_header.protocol_version() != protocol_version
            })
        {
            return Err(SyncLeapValidationError::WrongProtocolVersion);
        }

        let mut verified: Vec<BlockHash> = vec![self.trusted_block_header.block_hash()];

        while let Some(hash) = verified.pop() {
            if let Some(header) = headers.remove(&hash) {
                verified.push(*header.parent_hash());
                if let Some(validator_weights) = header.next_era_validator_weights() {
                    if let Some(era_sigs) = signatures.remove(&header.next_block_era_id()) {
                        for sigs in era_sigs {
                            if let Err(err) = utils::check_sufficient_block_signatures(
                                validator_weights,
                                chainspec.core_config.finality_threshold_fraction,
                                Some(sigs),
                            ) {
                                return Err(SyncLeapValidationError::HeadersNotSufficientlySigned(
                                    err,
                                ));
                            }
                            sigs.verify().map_err(SyncLeapValidationError::Crypto)?;
                            verified.push(sigs.block_hash);
                        }
                    }
                }
            }
        }

        // any orphaned headers == incomplete proof
        let incomplete_headers_proof = !headers.is_empty();
        // if trusted_ancestor_only == false, any orphaned signatures == incomplete proof
        let incomplete_signatures_proof = !signatures.is_empty();

        if incomplete_headers_proof || incomplete_signatures_proof {
            return Err(SyncLeapValidationError::IncompleteProof);
        }

        // defer cryptographic verification until last to avoid unnecessary computation
        for signed_header in &self.signed_block_headers {
            signed_header
                .validate()
                .map_err(SyncLeapValidationError::BlockWithMetadata)?;
        }

        Ok(())
    }
}
