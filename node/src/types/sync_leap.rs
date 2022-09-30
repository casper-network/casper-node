use std::{
    collections::BTreeMap,
    fmt::{self, Display, Formatter},
    iter,
};

use datasize::DataSize;
use itertools::Itertools;
use num_rational::Ratio;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::io::AsyncReadExt;

use casper_types::{crypto, system::auction::ValidatorWeights, EraId, PublicKey, U512};
use tracing::error;

use crate::{
    components::linear_chain::{self, BlockSignatureError},
    types::{
        block, error::BlockHeaderWithMetadataValidationError, BlockHash, BlockHeader,
        BlockHeaderWithMetadata, BlockSignatures, EraValidatorWeights, FetcherItem, Item, Tag,
        ValidatorMatrix,
    },
};

/// Headers and signatures required to prove that if a given trusted block hash is on the correct
/// chain, then so is a later header, which should be the most recent one according to the sender.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq, DataSize)]
pub(crate) struct SyncLeap {
    /// The header of the trusted block specified by hash by the requester.
    pub trusted_block_header: BlockHeader,
    /// The block headers of the trusted block's ancestors, back to the most recent switch block.
    /// Sorted from highest to lowest.
    pub trusted_ancestor_headers: Vec<BlockHeader>,
    /// The headers of all switch blocks known to the sender, after the trusted block but before
    /// their highest block, with signatures, plus the signed highest block.
    /// Sorted from lowest to highest.
    pub signed_block_headers: Vec<BlockHeaderWithMetadata>,
}

impl SyncLeap {
    pub(crate) fn highest_era(&self) -> EraId {
        // TODO - just use last signed block header
        self.signed_block_headers
            .iter()
            .map(|header_with_metadata| header_with_metadata.block_header.era_id())
            .max()
            .unwrap_or_else(|| self.trusted_block_header.era_id())
    }

    pub(crate) fn switch_blocks(&self) -> impl Iterator<Item = &BlockHeader> {
        self.trusted_ancestor_headers
            .iter()
            .chain(vec![&self.trusted_block_header])
            .chain(self.signed_block_headers.iter().map(|v| &v.block_header))
            .filter(|bh| bh.is_switch_block())
    }

    // Returns `true` if any new validator weight was registered.
    pub(crate) fn apply_validator_weights(&self, validator_matrix: &mut ValidatorMatrix) -> bool {
        let fault_tolerance_fraction = validator_matrix.fault_tolerance_threshold();
        self.switch_blocks().fold(false, |acc, switch| {
            if let Some(validator_weights) = switch.next_era_validator_weights() {
                validator_matrix.register_era_validator_weights(EraValidatorWeights::new(
                    switch.next_block_era_id(),
                    validator_weights.clone(),
                    fault_tolerance_fraction,
                )) || acc
            } else {
                acc
            }
        })
    }

    pub(crate) fn highest_block_height(&self) -> u64 {
        // TODO - just use last signed block header
        self.signed_block_headers
            .iter()
            .map(|header_with_metadata| header_with_metadata.block_header.height())
            .max()
            .unwrap_or_else(|| self.trusted_block_header.height())
    }

    pub(crate) fn highest_block_header(&self) -> Option<BlockHeaderWithMetadata> {
        let highest = self.highest_block_height();
        self.signed_block_headers
            .iter()
            .cloned()
            .find_or_first(|block_header_with_metadata| {
                block_header_with_metadata.block_header.height() == highest
            })
    }

    pub(crate) fn highest_block_signatures(&self) -> Option<BlockSignatures> {
        match self.highest_block_header() {
            None => None,
            Some(v) => Some(v.block_signatures),
        }
    }

    pub(crate) fn validators_of_highest_block(&self) -> Option<ValidatorWeights> {
        let highest = self.highest_block_height();
        if highest == 0 {
            // There's just one genesis block in the chain.
            return Some(
                self.trusted_block_header
                    .next_era_validator_weights()
                    .expect("block of height 0 must have next era validator weights")
                    .clone(),
            );
        }

        if self.signed_block_headers.is_empty()
            || (!self.trusted_block_header.is_switch_block()
                && self.signed_block_headers.len() == 1)
        {
            if let Some(trusted_ancestor_header) = self.trusted_ancestor_headers.last() {
                if let Some(next_era_validator_weights) =
                    trusted_ancestor_header.next_era_validator_weights()
                {
                    return Some(next_era_validator_weights.clone());
                }
            }
            return None;
        }

        if self.trusted_block_header.is_switch_block() && self.signed_block_headers.len() == 1 {
            if let Some(next_era_validator_weights) =
                self.trusted_block_header.next_era_validator_weights()
            {
                return Some(next_era_validator_weights.clone());
            }
            error!(
                block_hash = %self.trusted_block_header.hash(),
                "switch block without next era validator weights"
            );
            return None;
        }

        if let Some(signed_block_header) = self.signed_block_headers.iter().rev().nth(1) {
            if let Some(next_era_validator_weights) = signed_block_header
                .block_header
                .next_era_validator_weights()
            {
                return Some(next_era_validator_weights.clone());
            }
        }
        None
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

        // The header chain should only go back until it hits _one_ switch block.
        for header in self.trusted_ancestor_headers.iter().rev().skip(1) {
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
            .rev()
            .chain(iter::once(&self.trusted_block_header))
            .tuple_windows()
        {
            if *child_header.parent_hash() != parent_header.hash() {
                return Err(SyncLeapValidationError::HeadersNotContiguous);
            }
        }

        // The oldest provided block must be a switch block, so that we know the validators
        // who are expected to sign later blocks.
        let oldest_header = self
            .trusted_ancestor_headers
            .last()
            .unwrap_or(&self.trusted_block_header);
        assert!(oldest_header.is_switch_block());

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
                validator_weights,
                *finality_threshold_fraction,
                Some(&signed_header.block_signatures),
            ) {
                Ok(()) => (),
                Err(err) => return Err(SyncLeapValidationError::HeadersNotSufficientlySigned(err)),
            }
            signed_header
                .validate()
                .map_err(SyncLeapValidationError::BlockWithMetadata)?;

            signed_header
                .block_signatures
                .verify()
                .map_err(SyncLeapValidationError::Crypto)?;

            if let Some(next_validator_weights) =
                signed_header.block_header.next_era_validator_weights()
            {
                validator_weights = next_validator_weights;
                validators_era_id = signed_header.block_header.era_id().successor();
            }
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
            let (validator_secret_key, validator_public_key) = generate_ed25519_keypair();
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
            block_signatures: BlockSignatures::new(block_header.hash(), block_header.era_id()),
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
            BlockHeaderSpec::new(10),
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
            BlockHeaderSpec::new_switch(10),
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
        //   - signed_block_headers contains just one entry (meaning, it contains a non-switch tip
        //     only)
        // Take validator weights from the least recent entry in `trusted_ancestor_header`, which is
        // a switch block of interest.
        let sync_leap = create_sync_leap(
            &mut rng,
            BlockHeaderSpec::new(10),
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
        //   - signed_block_headers contains just one entry (meaning, it contains a non-switch tip
        //     only)
        // Take validator weights from the trusted block header, which is a switch block of
        // interest.
        let sync_leap = create_sync_leap(
            &mut rng,
            BlockHeaderSpec::new_switch(10),
            &[
                BlockHeaderSpec::new(16),
                BlockHeaderSpec::new(15),
                BlockHeaderSpec::new(14),
                BlockHeaderSpec::new_switch(13),
            ],
            &[BlockHeaderSpec::new(20)],
        );
        assert_validator_weights(10, sync_leap);

        // Case 4:
        //  - signed_block_headers is empty (meaning, we were asked for a tip)
        // Take validator weights from the least recent entry in `trusted_ancestor_header`, which is
        // a switch block of interest.
        let sync_leap = create_sync_leap(
            &mut rng,
            BlockHeaderSpec::new_switch(10),
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
