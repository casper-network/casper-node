use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
    iter,
    sync::Arc,
};

use datasize::DataSize;
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
#[cfg(test)]
use casper_types::system::auction::ValidatorWeights;

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
    #[error("Too many trusted ancestor headers: no more than one era's worth is needed.")]
    TooManyTrustedAncestors,
    #[error("Signed block headers present despite trusted_ancestor_only flag.")]
    UnexpectedSignedBlockHeaders,
}

/// Identifier for a SyncLeap.
#[derive(Debug, Serialize, Deserialize, Copy, Clone, Hash, PartialEq, Eq, DataSize)]
pub(crate) enum SyncLeapIdentifier {
    Forward {
        block_hash: BlockHash,
        number_of_switch_blocks_needed: u8,
    },
    Historical {
        block_hash: BlockHash,
    },
}

impl SyncLeapIdentifier {
    pub(crate) fn sync_to_tip(block_hash: BlockHash, number_of_switch_blocks_needed: u8) -> Self {
        SyncLeapIdentifier::Forward {
            block_hash,
            number_of_switch_blocks_needed,
        }
    }

    pub(crate) fn sync_to_historical(block_hash: BlockHash) -> Self {
        SyncLeapIdentifier::Historical { block_hash }
    }

    pub(crate) fn block_hash(&self) -> BlockHash {
        match self {
            SyncLeapIdentifier::Forward { block_hash, .. }
            | SyncLeapIdentifier::Historical { block_hash } => *block_hash,
        }
    }
}

impl Display for SyncLeapIdentifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            SyncLeapIdentifier::Forward {
                block_hash,
                number_of_switch_blocks_needed,
            } => {
                write!(
                    f,
                    "{} with {} switch blocks",
                    block_hash, number_of_switch_blocks_needed
                )
            }
            SyncLeapIdentifier::Historical { block_hash } => {
                write!(f, "{} with ancestors only", block_hash)
            }
        }
    }
}

/// Headers and signatures required to prove that if a given trusted block hash is on the correct
/// chain, then so is a later header, which should be the most recent one according to the sender.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, DataSize)]
pub(crate) struct SyncLeap {
    /// If `None`, this is a historical sync leap, and `signed_block_headers` should not be
    /// populated.
    ///
    /// If `Some`, this defines the number of switch blocks which should be included in the leap.
    pub number_of_switch_blocks_needed: Option<u8>,
    /// The header of the trusted block specified by hash by the requester.
    pub trusted_block_header: BlockHeader,
    /// The block headers of the trusted block's ancestors, back to a switch block.
    ///
    /// If `number_of_switch_blocks_needed` is `None`, the switch block will be the most recent
    /// before the trusted header.
    ///
    /// If not, the switch block will be the most recent before the trusted header such that there
    /// are at least `number_of_switch_blocks_needed` total switch blocks included between this
    /// field and `signed_block_headers`.
    pub trusted_ancestor_headers: Vec<BlockHeader>,
    /// The headers of all switch blocks known to the sender, after the trusted block but before
    /// their highest block, with signatures, plus the signed highest block.
    ///
    /// This field should be empty if `number_of_switch_blocks_needed` is `None`.
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

    pub(crate) fn headers(&self) -> impl Iterator<Item = &BlockHeader> {
        iter::once(&self.trusted_block_header)
            .chain(&self.trusted_ancestor_headers)
            .chain(self.signed_block_headers.iter().map(|sh| &sh.block_header))
    }

    #[cfg(test)]
    pub(crate) fn validators_of_highest_block(&self) -> Option<&ValidatorWeights> {
        let (highest_header, _) = self.highest_block_header();
        self.switch_blocks()
            .find(|switch_block| switch_block.next_block_era_id() == highest_header.era_id())
            .and_then(BlockHeader::next_era_validator_weights)
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
        let block_hash = self.trusted_block_header.block_hash();
        match self.number_of_switch_blocks_needed {
            Some(number_of_switch_blocks_needed) => SyncLeapIdentifier::Forward {
                block_hash,
                number_of_switch_blocks_needed,
            },
            None => SyncLeapIdentifier::Historical { block_hash },
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
        if self.trusted_ancestor_headers.len() as u64 > chainspec.max_blocks_per_era() {
            return Err(SyncLeapValidationError::TooManyTrustedAncestors);
        }
        if self.number_of_switch_blocks_needed.is_none() && !self.signed_block_headers.is_empty() {
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
                                chainspec.highway_config.finality_threshold_fraction,
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

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use casper_types::{
        generate_ed25519_keypair, testing::TestRng, EraId, ProtocolVersion, PublicKey, U512,
    };

    use crate::types::{Block, BlockHeader, BlockHeaderWithMetadata, BlockSignatures, SyncLeap};

    struct BlockHeaderSpec {
        height: u64,
        is_switch: bool,
        validator_weights: BTreeMap<PublicKey, U512>,
    }

    impl BlockHeaderSpec {
        fn new(height: u64) -> Self {
            Self {
                height,
                is_switch: false,
                validator_weights: BTreeMap::new(),
            }
        }
        fn new_switch(height: u64) -> Self {
            // For the sake of making the assertions simpler, we'll match validator weight
            // with the block height.
            let mut validator_weights = BTreeMap::new();
            let (_validator_secret_key, validator_public_key) = generate_ed25519_keypair();
            validator_weights.insert(validator_public_key, U512::from(height));

            Self {
                height,
                is_switch: true,
                validator_weights,
            }
        }
    }

    fn header_with_height(rng: &mut TestRng, spec: &BlockHeaderSpec) -> BlockHeader {
        Block::random_with_specifics_and_parent_and_validator_weights(
            rng,
            EraId::from(0),
            spec.height,
            ProtocolVersion::default(),
            spec.is_switch,
            None,
            None,
            spec.validator_weights.clone(),
        )
        .take_header()
    }

    fn into_header_with_metadata(block_header: BlockHeader) -> BlockHeaderWithMetadata {
        BlockHeaderWithMetadata {
            block_signatures: BlockSignatures::new(
                block_header.block_hash(),
                block_header.era_id(),
            ),
            block_header,
        }
    }

    fn create_sync_leap(
        rng: &mut TestRng,
        trusted_block_header_spec: BlockHeaderSpec,
        trusted_ancestor_headers_spec: &[BlockHeaderSpec],
        signed_block_headers_spec: &[BlockHeaderSpec],
    ) -> SyncLeap {
        let trusted_block_header = header_with_height(rng, &trusted_block_header_spec);

        let trusted_ancestor_headers = trusted_ancestor_headers_spec
            .iter()
            .map(|spec| header_with_height(rng, spec))
            .collect();

        let signed_block_headers = signed_block_headers_spec
            .iter()
            .map(|spec| into_header_with_metadata(header_with_height(rng, spec)))
            .collect();

        SyncLeap {
            number_of_switch_blocks_needed: Some(1),
            trusted_block_header,
            trusted_ancestor_headers,
            signed_block_headers,
        }
    }

    fn assert_validator_weights(expected: u64, sync_leap: SyncLeap) {
        assert_eq!(
            &U512::from(expected),
            sync_leap
                .validators_of_highest_block()
                .unwrap()
                .iter()
                .next()
                .unwrap()
                .1
        );
    }

    #[test]
    fn gets_highest_block_height() {
        let mut rng = TestRng::new();

        let sync_leap = create_sync_leap(
            &mut rng,
            BlockHeaderSpec::new(10),
            &[],
            &[
                BlockHeaderSpec::new(12),
                BlockHeaderSpec::new(15),
                BlockHeaderSpec::new(16),
            ],
        );
        assert_eq!(16, sync_leap.highest_block_height());

        let sync_leap = create_sync_leap(&mut rng, BlockHeaderSpec::new(10), &[], &[]);
        assert_eq!(10, sync_leap.highest_block_height());
    }

    #[test]
    #[should_panic]
    fn should_fail_getting_validators_of_highest_block_when_block_0_is_not_switch() {
        let mut rng = TestRng::new();

        // Fails, because block 13 is expected to be a switch block when signed_block_headers is
        // empty.
        let sync_leap = create_sync_leap(&mut rng, BlockHeaderSpec::new(0), &[], &[]);
        let _ = sync_leap.validators_of_highest_block();
    }

    #[test]
    fn should_fail_getting_validators_of_highest_block_on_malformed_sync_leap() {
        let mut rng = TestRng::new();

        // Fails, because block 13 is expected to be a switch block when
        // - trusted header is not a switch block
        // - there's a single entry in signed_block_headers
        let sync_leap = create_sync_leap(
            &mut rng,
            BlockHeaderSpec::new(17),
            &[
                BlockHeaderSpec::new(16),
                BlockHeaderSpec::new(15),
                BlockHeaderSpec::new(14),
                BlockHeaderSpec::new(13),
            ],
            &[BlockHeaderSpec::new(20)],
        );
        assert!(sync_leap.validators_of_highest_block().is_none());

        // Fails, because block 13 is expected to be a switch block when signed_block_headers is
        // empty.
        let sync_leap = create_sync_leap(
            &mut rng,
            BlockHeaderSpec::new_switch(17),
            &[
                BlockHeaderSpec::new(16),
                BlockHeaderSpec::new(15),
                BlockHeaderSpec::new(14),
                BlockHeaderSpec::new(13),
            ],
            &[],
        );
        assert!(sync_leap.validators_of_highest_block().is_none());

        // Fails, because block 20 is expected to be a switch block when signed_block_headers has
        // more than a single entry.
        let sync_leap = create_sync_leap(
            &mut rng,
            BlockHeaderSpec::new(10),
            &[
                BlockHeaderSpec::new(9),
                BlockHeaderSpec::new(8),
                BlockHeaderSpec::new(7),
                BlockHeaderSpec::new(6),
            ],
            &[
                BlockHeaderSpec::new(17),
                BlockHeaderSpec::new(18),
                BlockHeaderSpec::new(19),
                BlockHeaderSpec::new(20),
                BlockHeaderSpec::new(21),
            ],
        );
        assert!(sync_leap.validators_of_highest_block().is_none());
    }

    #[test]
    fn gets_validators_of_highest_block() {
        let mut rng = TestRng::new();

        // Case 1:
        // At genesis we always get validators from trusted_block_header
        let sync_leap = create_sync_leap(&mut rng, BlockHeaderSpec::new_switch(0), &[], &[]);
        assert_validator_weights(0, sync_leap);

        // Case 2:
        //   - Trusted header IS NOT a switch block
        //   - signed_block_headers contains just one entry (meaning, it contains a tip only)
        // Take validator weights from the least recent entry in `trusted_ancestor_header`, which is
        // a switch block of interest.
        let sync_leap = create_sync_leap(
            &mut rng,
            BlockHeaderSpec::new(17),
            &[
                BlockHeaderSpec::new(16),
                BlockHeaderSpec::new(15),
                BlockHeaderSpec::new(14),
                BlockHeaderSpec::new_switch(13),
            ],
            &[BlockHeaderSpec::new(20)],
        );
        assert_validator_weights(13, sync_leap);

        // Case 3:
        //   - Trusted header IS a switch block
        //   - signed_block_headers contains just one entry (meaning, it contains a tip only)
        // Take validator weights from the trusted block header, which is a switch block of
        // interest.
        let sync_leap = create_sync_leap(
            &mut rng,
            BlockHeaderSpec::new_switch(17),
            &[
                BlockHeaderSpec::new(16),
                BlockHeaderSpec::new(15),
                BlockHeaderSpec::new(14),
                BlockHeaderSpec::new_switch(13),
            ],
            &[BlockHeaderSpec::new(20)],
        );
        assert_validator_weights(17, sync_leap);

        // Case 4:
        //  - signed_block_headers is empty (meaning, we were asked for a tip)
        // Take validator weights from the least recent entry in `trusted_ancestor_header`, which is
        // a switch block of interest.
        let sync_leap = create_sync_leap(
            &mut rng,
            BlockHeaderSpec::new_switch(17),
            &[
                BlockHeaderSpec::new(16),
                BlockHeaderSpec::new(15),
                BlockHeaderSpec::new(14),
                BlockHeaderSpec::new_switch(13),
            ],
            &[],
        );
        assert_validator_weights(13, sync_leap);

        // Case 5:
        //  - there's more than 1 item in signed_block_headers
        // Take validator weights from the penultimate entry in `signed_block_headers`, which is a
        // switch block of interest, as the last entry must be a tip.
        let sync_leap = create_sync_leap(
            &mut rng,
            BlockHeaderSpec::new(10),
            &[
                BlockHeaderSpec::new(9),
                BlockHeaderSpec::new(8),
                BlockHeaderSpec::new(7),
                BlockHeaderSpec::new(6),
            ],
            &[
                BlockHeaderSpec::new(17),
                BlockHeaderSpec::new(18),
                BlockHeaderSpec::new(19),
                BlockHeaderSpec::new_switch(20),
                BlockHeaderSpec::new(21),
            ],
        );
        assert_validator_weights(20, sync_leap);
    }
}
