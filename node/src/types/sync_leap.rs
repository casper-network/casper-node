use std::{
    collections::{BTreeMap, HashSet},
    fmt::{self, Display, Formatter},
    iter,
};

use datasize::DataSize;
use itertools::Itertools;
use num_rational::Ratio;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use casper_types::{crypto, EraId};
use tracing::error;

use crate::{
    components::fetcher::{FetchItem, Tag},
    types::{
        error::BlockHeaderWithMetadataValidationError, BlockHash, BlockHeader,
        BlockHeaderWithMetadata, BlockSignatures, Chainspec, EraValidatorWeights,
    },
    utils::{self, BlockSignatureError},
};

use super::{chainspec::GlobalStateUpdate, ActivationPoint};

#[derive(Error, Debug)]
pub(crate) enum SyncLeapValidationError {
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
    #[error(
        "Only the last trusted ancestor is allowed to be a switch block or the genesis block."
    )]
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
    pub(crate) fn era_validator_weights(
        &self,
        fault_tolerance_fraction: Ratio<u64>,
    ) -> impl Iterator<Item = EraValidatorWeights> + '_ {
        let switch_block_heights: HashSet<_> = self
            .switch_blocks_headers()
            .map(BlockHeader::height)
            .collect();
        self.switch_blocks_headers()
            .find(|block_header| block_header.is_genesis())
            .into_iter()
            .flat_map(move |block_header| {
                Some(EraValidatorWeights::new(
                    EraId::default(),
                    block_header.next_era_validator_weights().cloned()?,
                    fault_tolerance_fraction,
                ))
            })
            .chain(
                self.switch_blocks_headers()
                    // filter out switch blocks preceding immediate switch blocks - we don't want
                    // to read the era validators directly from them, as they might have been
                    // altered by the upgrade, we'll get them from the blocks' global states
                    // instead
                    .filter(move |block_header| {
                        !switch_block_heights.contains(&(block_header.height() + 1))
                    })
                    .flat_map(move |block_header| {
                        Some(EraValidatorWeights::new(
                            block_header.next_block_era_id(),
                            block_header.next_era_validator_weights().cloned()?,
                            fault_tolerance_fraction,
                        ))
                    }),
            )
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

    pub(crate) fn switch_blocks_headers(&self) -> impl Iterator<Item = &BlockHeader> {
        self.headers().filter(|header| header.is_switch_block())
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

#[derive(Clone, DataSize, Debug, Eq, PartialEq, Serialize)]
pub(crate) struct SyncLeapValidationMetaData {
    recent_era_count: u64,
    activation_point: ActivationPoint,
    global_state_update: Option<GlobalStateUpdate>,
    #[data_size(skip)]
    finality_threshold_fraction: Ratio<u64>,
}

impl SyncLeapValidationMetaData {
    #[cfg(any(feature = "testing", test))]
    pub(crate) fn new(
        recent_era_count: u64,
        activation_point: ActivationPoint,
        global_state_update: Option<GlobalStateUpdate>,
        finality_threshold_fraction: Ratio<u64>,
    ) -> Self {
        Self {
            recent_era_count,
            activation_point,
            global_state_update,
            finality_threshold_fraction,
        }
    }

    pub(crate) fn from_chainspec(chainspec: &Chainspec) -> Self {
        Self {
            recent_era_count: chainspec.core_config.recent_era_count(),
            activation_point: chainspec.protocol_config.activation_point,
            global_state_update: chainspec.protocol_config.global_state_update.clone(),
            finality_threshold_fraction: chainspec.core_config.finality_threshold_fraction,
        }
    }
}

impl FetchItem for SyncLeap {
    type Id = SyncLeapIdentifier;
    type ValidationError = SyncLeapValidationError;
    type ValidationMetadata = SyncLeapValidationMetaData;

    const TAG: Tag = Tag::SyncLeap;

    fn fetch_id(&self) -> Self::Id {
        SyncLeapIdentifier {
            block_hash: self.trusted_block_header.block_hash(),
            trusted_ancestor_only: self.trusted_ancestor_only,
        }
    }

    fn validate(
        &self,
        validation_metadata: &SyncLeapValidationMetaData,
    ) -> Result<(), Self::ValidationError> {
        if self.trusted_ancestor_headers.is_empty() && self.trusted_block_header.height() > 0 {
            return Err(SyncLeapValidationError::MissingTrustedAncestors);
        }
        if self.signed_block_headers.len() as u64
            > validation_metadata.recent_era_count.saturating_add(1)
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
            if !last_ancestor.is_switch_block() && !last_ancestor.is_genesis() {
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

        let mut headers_with_sufficient_finality: Vec<BlockHash> =
            vec![self.trusted_block_header.block_hash()];

        while let Some(hash) = headers_with_sufficient_finality.pop() {
            if let Some(header) = headers.remove(&hash) {
                headers_with_sufficient_finality.push(*header.parent_hash());
                if let Some(mut validator_weights) = header.next_era_validator_weights() {
                    // If this is a switch block right before the upgrade to the current protocol
                    // version, and if this upgrade changes the validator set, use the validator
                    // weights from the chainspec.

                    dbg!(&header.next_block_era_id());
                    dbg!(&validation_metadata.activation_point.era_id());

                    if header.next_block_era_id() == validation_metadata.activation_point.era_id() {
                        if let Some(updated_weights) = validation_metadata
                            .global_state_update
                            .as_ref()
                            .and_then(|update| update.validators.as_ref())
                        {
                            validator_weights = updated_weights
                        }
                    }

                    dbg!(&validator_weights);

                    if let Some(era_sigs) = signatures.remove(&header.next_block_era_id()) {
                        for sigs in era_sigs {
                            if let Err(err) = utils::check_sufficient_block_signatures(
                                validator_weights,
                                validation_metadata.finality_threshold_fraction,
                                Some(sigs),
                            ) {
                                return Err(SyncLeapValidationError::HeadersNotSufficientlySigned(
                                    err,
                                ));
                            }
                            headers_with_sufficient_finality.push(sigs.block_hash);
                        }
                    }
                }
            }
        }

        // any orphaned headers == incomplete proof
        let incomplete_headers_proof = !headers.is_empty();
        // any orphaned signatures == incomplete proof
        let incomplete_signatures_proof = !signatures.is_empty();

        if incomplete_headers_proof || incomplete_signatures_proof {
            return Err(SyncLeapValidationError::IncompleteProof);
        }

        for signed_header in &self.signed_block_headers {
            signed_header
                .validate()
                .map_err(SyncLeapValidationError::BlockWithMetadata)?;
        }

        // defer cryptographic verification until last to avoid unnecessary computation
        for signed_header in &self.signed_block_headers {
            signed_header
                .block_signatures
                .verify()
                .map_err(SyncLeapValidationError::Crypto)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    // The `FetchItem::<SyncLeap>::validate()` function can potentially return the
    // `SyncLeapValidationError::BlockWithMetadata` error as a result of calling
    // `BlockHeaderWithMetadata::validate()`, but in practice this will always be detected earlier
    // as an `SyncLeapValidationError::IncompleteProof` error. Hence, there is no explicit test for
    // `SyncLeapValidationError::BlockWithMetadata`.

    use std::{
        collections::{BTreeMap, BTreeSet},
        iter,
    };

    use casper_types::{crypto, testing::TestRng, PublicKey, SecretKey, Signature};
    use itertools::Itertools;
    use num_rational::Ratio;

    use crate::{
        components::fetcher::FetchItem,
        tests::{TestBlockIterator, TestChainSpec},
        types::{
            chainspec::GlobalStateUpdate, sync_leap::SyncLeapValidationError, ActivationPoint,
            Block, BlockHeader, BlockHeaderWithMetadata, BlockSignatures, FinalitySignature,
            SyncLeapValidationMetaData,
        },
        utils::BlockSignatureError,
    };

    use super::SyncLeap;

    fn _dump_chain(chain: &[Block]) {
        for block in chain {
            println!(
                "block={} - era={} - is_switch={} - next_era_validator_weights={}",
                block.header().height(),
                block.header().era_id(),
                block.header().is_switch_block(),
                block.header().next_era_validator_weights().is_some()
            );
        }
    }

    fn _dump_sync_leap(sync_leap: &SyncLeap) {
        println!(
            "trusted_block_header={}, trusted_ancestor_headers={}, signed_block_headers={}",
            sync_leap.trusted_block_header.height(),
            sync_leap
                .trusted_ancestor_headers
                .iter()
                .map(|header| header.height())
                .join(","),
            sync_leap
                .signed_block_headers
                .iter()
                .map(|header_with_metadata| header_with_metadata.block_header.height())
                .join(",")
        );
    }

    fn make_signed_block_header_from_height(
        height: usize,
        test_chain: &[Block],
        validators: &[(SecretKey, PublicKey)],
        add_proofs: bool,
    ) -> BlockHeaderWithMetadata {
        let header = test_chain.get(height).unwrap().header().clone();
        make_signed_block_header_from_header(&header, validators, add_proofs)
    }

    fn make_signed_block_header_from_header(
        block_header: &BlockHeader,
        validators: &[(SecretKey, PublicKey)],
        add_proofs: bool,
    ) -> BlockHeaderWithMetadata {
        let hash = block_header.block_hash();
        let era_id = block_header.era_id();
        let mut block_signatures = BlockSignatures::new(hash, era_id);
        validators.iter().for_each(|(secret_key, public_key)| {
            let finality_signature =
                FinalitySignature::create(hash, era_id, &secret_key, public_key.clone());
            if add_proofs {
                block_signatures.insert_proof(public_key.clone(), finality_signature.signature);
            }
        });

        BlockHeaderWithMetadata {
            block_header: block_header.clone(),
            block_signatures,
        }
    }

    fn make_test_sync_leap(
        rng: &mut TestRng,
        switch_blocks: &[u64],
        query: usize,
        trusted_ancestor_headers: &[usize],
        signed_block_headers: &[usize],
        add_proofs: bool,
    ) -> SyncLeap {
        let validators: Vec<_> = iter::repeat_with(|| crypto::generate_ed25519_keypair())
            .take(2)
            .collect();
        let mut test_chain_spec = TestChainSpec::new(
            rng,
            Some(switch_blocks.iter().copied().collect()),
            &validators,
        );
        let test_chain: Vec<_> = test_chain_spec.iter().take(12).collect();

        let trusted_block_header = test_chain.get(query).unwrap().header().clone();

        let trusted_ancestor_headers: Vec<_> = trusted_ancestor_headers
            .iter()
            .map(|height| test_chain.get(*height).unwrap().header().clone())
            .collect();

        let signed_block_headers: Vec<_> = signed_block_headers
            .iter()
            .map(|height| {
                make_signed_block_header_from_height(*height, &test_chain, &validators, add_proofs)
            })
            .collect();

        SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header,
            trusted_ancestor_headers,
            signed_block_headers,
        }
    }

    fn test_sync_leap_validation_metadata() -> SyncLeapValidationMetaData {
        let unbonding_delay = 7;
        let auction_delay = 1;
        let activation_point = ActivationPoint::EraId(3000.into());
        let finality_threshold_fraction = Ratio::new(1, 3);

        SyncLeapValidationMetaData::new(
            unbonding_delay - auction_delay, // As per `CoreConfig::recent_era_count()`.
            activation_point,
            None,
            finality_threshold_fraction,
        )
    }

    #[test]
    fn should_validate_correct_sync_leap() {
        // Chain
        // 0   1   2   3   4   5   6   7   8   9   10   11
        // S           S           S           S
        let switch_blocks = [0, 3, 6, 9];
        let validation_metadata = test_sync_leap_validation_metadata();

        let mut rng = TestRng::new();

        // Querying for a non-switch block.
        let query = 5;
        let trusted_ancestor_headers = [4, 3];
        let signed_block_headers = [6, 9, 11];
        let add_proofs = true;
        let sync_leap = make_test_sync_leap(
            &mut rng,
            &switch_blocks,
            query,
            &trusted_ancestor_headers,
            &signed_block_headers,
            add_proofs,
        );

        let result = sync_leap.validate(&validation_metadata);
        assert!(result.is_ok());

        // Querying for a switch block.
        let query = 6;
        let trusted_ancestor_headers = [5, 4, 3];
        let signed_block_headers = [9, 11];
        let add_proofs = true;
        let sync_leap = make_test_sync_leap(
            &mut rng,
            &switch_blocks,
            query,
            &trusted_ancestor_headers,
            &signed_block_headers,
            add_proofs,
        );

        let result = sync_leap.validate(&validation_metadata);
        assert!(result.is_ok());
    }

    #[test]
    fn should_check_trusted_ancestors() {
        let mut rng = TestRng::new();
        let validation_metadata = test_sync_leap_validation_metadata();

        // Trusted ancestors can't be empty when trusted block height is greater than 0.
        let mut block = Block::random(&mut rng);
        block.header_mut().set_height(1);

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: block.take_header(),
            trusted_ancestor_headers: Default::default(),
            signed_block_headers: Default::default(),
        };
        let result = sync_leap.validate(&validation_metadata);
        assert!(matches!(
            result,
            Err(SyncLeapValidationError::MissingTrustedAncestors)
        ));

        // When trusted block height is 0 and trusted ancestors are empty, validate
        // should yield a result different than `SyncLeapValidationError::MissingTrustedAncestors`.
        let mut block = Block::random(&mut rng);
        block.header_mut().set_height(0);

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: block.take_header(),
            trusted_ancestor_headers: Default::default(),
            signed_block_headers: Default::default(),
        };
        let result = sync_leap.validate(&validation_metadata);
        assert!(!matches!(
            result,
            Err(SyncLeapValidationError::MissingTrustedAncestors)
        ));
    }

    #[test]
    fn should_check_signed_block_headers_size() {
        let mut rng = TestRng::new();
        let validation_metadata = test_sync_leap_validation_metadata();

        let max_allowed_size = validation_metadata.recent_era_count + 1;

        // Max allowed size should NOT trigger the `TooManySwitchBlocks` error.
        let generated_block_count = max_allowed_size;

        let mut block = Block::random(&mut rng);
        block.header_mut().set_height(0);
        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: block.take_header(),
            trusted_ancestor_headers: Default::default(),
            signed_block_headers: std::iter::repeat_with(|| {
                let block = Block::random(&mut rng);
                let hash = block.hash();
                BlockHeaderWithMetadata {
                    block_header: block.header().clone(),
                    block_signatures: BlockSignatures::new(*hash, 0.into()),
                }
            })
            .take(generated_block_count as usize)
            .collect(),
        };
        let result = sync_leap.validate(&validation_metadata);
        assert!(!matches!(
            result,
            Err(SyncLeapValidationError::TooManySwitchBlocks)
        ));

        // Generating one more block should trigger the `TooManySwitchBlocks` error.
        let generated_block_count = max_allowed_size + 1;

        let mut block = Block::random(&mut rng);
        block.header_mut().set_height(0);
        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: block.take_header(),
            trusted_ancestor_headers: Default::default(),
            signed_block_headers: std::iter::repeat_with(|| {
                let block = Block::random(&mut rng);
                let hash = block.hash();
                BlockHeaderWithMetadata {
                    block_header: block.header().clone(),
                    block_signatures: BlockSignatures::new(*hash, 0.into()),
                }
            })
            .take(generated_block_count as usize)
            .collect(),
        };
        let result = sync_leap.validate(&validation_metadata);
        assert!(matches!(
            result,
            Err(SyncLeapValidationError::TooManySwitchBlocks)
        ));
    }

    #[test]
    fn should_detect_unsorted_trusted_ancestors() {
        let mut rng = TestRng::new();
        let validation_metadata = test_sync_leap_validation_metadata();

        // Test block iterator produces blocks in order, however, the `trusted_ancestor_headers` is
        // expected to be sorted backwards (from the most recent ancestor back to the switch block).
        // Therefore, the generated blocks should cause the `TrustedAncestorsNotSorted` error to be
        // triggered.
        let mut block = Block::random(&mut rng);
        block.header_mut().set_height(0);
        let block_iterator =
            TestBlockIterator::new(block.clone(), &mut rng, None, Default::default());

        let trusted_ancestor_headers = block_iterator
            .take(3)
            .map(|block| block.take_header())
            .collect();

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: block.take_header(),
            trusted_ancestor_headers,
            signed_block_headers: Default::default(),
        };
        let result = sync_leap.validate(&validation_metadata);
        assert!(matches!(
            result,
            Err(SyncLeapValidationError::TrustedAncestorsNotSorted)
        ));

        // Single trusted ancestor header it should never trigger the `TrustedAncestorsNotSorted`
        // error.
        let mut block = Block::random(&mut rng);
        block.header_mut().set_height(0);
        let block_iterator =
            TestBlockIterator::new(block.clone(), &mut rng, None, Default::default());

        let trusted_ancestor_headers = block_iterator
            .take(1)
            .map(|block| block.take_header())
            .collect();

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: block.take_header(),
            trusted_ancestor_headers,
            signed_block_headers: Default::default(),
        };
        let result = sync_leap.validate(&validation_metadata);
        assert!(!matches!(
            result,
            Err(SyncLeapValidationError::TrustedAncestorsNotSorted)
        ));
    }

    #[test]
    fn should_detect_missing_ancestor_switch_block() {
        let mut rng = TestRng::new();
        let validation_metadata = test_sync_leap_validation_metadata();

        // Make sure `TestBlockIterator` creates no switch blocks.
        let switch_blocks = None;

        let mut block = Block::random(&mut rng);
        block.header_mut().set_height(0);
        let block_iterator =
            TestBlockIterator::new(block.clone(), &mut rng, switch_blocks, Default::default());

        let trusted_ancestor_headers: Vec<_> = block_iterator
            .take(3)
            .map(|block| block.take_header())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: block.take_header(),
            trusted_ancestor_headers,
            signed_block_headers: Default::default(),
        };
        let result = sync_leap.validate(&validation_metadata);
        assert!(matches!(
            result,
            Err(SyncLeapValidationError::MissingAncestorSwitchBlock)
        ));
    }

    #[test]
    fn should_detect_unexpected_ancestor_switch_block() {
        // Chain
        // 0   1   2   3   4   5   6   7   8   9   10   11
        // S       S   S           S           S
        let switch_blocks = [0, 2, 3, 6, 9];
        let validation_metadata = test_sync_leap_validation_metadata();

        let mut rng = TestRng::new();

        // Intentionally include two consecutive switch blocks (3, 2) in the
        // `trusted_ancestor_headers`, which should trigger the error.
        let trusted_ancestor_headers = [4, 3, 2];

        let query = 5;
        let signed_block_headers = [6, 9, 11];
        let add_proofs = true;
        let sync_leap = make_test_sync_leap(
            &mut rng,
            &switch_blocks,
            query,
            &trusted_ancestor_headers,
            &signed_block_headers,
            add_proofs,
        );

        let result = sync_leap.validate(&validation_metadata);
        assert!(matches!(
            result,
            Err(SyncLeapValidationError::UnexpectedAncestorSwitchBlock)
        ));
    }

    #[test]
    fn should_detect_unexpected_signed_block_header() {
        // Chain
        // 0   1   2   3   4   5   6   7   8   9   10   11
        // S           S           S           S
        let switch_blocks = [0, 3, 6, 9];
        let validation_metadata = test_sync_leap_validation_metadata();

        let mut rng = TestRng::new();

        let query = 5;
        let trusted_ancestor_headers = [4, 3];
        let signed_block_headers = [6, 9, 11];
        let add_proofs = true;
        let mut sync_leap = make_test_sync_leap(
            &mut rng,
            &switch_blocks,
            query,
            &trusted_ancestor_headers,
            &signed_block_headers,
            add_proofs,
        );

        // When `trusted_ancestor_only` we expect an error when `signed_block_headers` is not empty.
        sync_leap.trusted_ancestor_only = true;

        let result = sync_leap.validate(&validation_metadata);
        assert!(matches!(
            result,
            Err(SyncLeapValidationError::UnexpectedSignedBlockHeaders)
        ));
    }

    #[test]
    fn should_detect_not_sufficiently_signed_headers() {
        // Chain
        // 0   1   2   3   4   5   6   7   8   9   10   11
        // S           S           S           S
        let switch_blocks = [0, 3, 6, 9];
        let validation_metadata = test_sync_leap_validation_metadata();

        let mut rng = TestRng::new();

        let query = 5;
        let trusted_ancestor_headers = [4, 3];
        let signed_block_headers = [6, 9, 11];
        let add_proofs = false;
        let sync_leap = make_test_sync_leap(
            &mut rng,
            &switch_blocks,
            query,
            &trusted_ancestor_headers,
            &signed_block_headers,
            add_proofs,
        );

        let result = sync_leap.validate(&validation_metadata);
        assert!(
            matches!(result, Err(SyncLeapValidationError::HeadersNotSufficientlySigned(inner))
             if matches!(&inner, BlockSignatureError::InsufficientWeightForFinality{
                trusted_validator_weights: _,
                block_signatures: _,
                signature_weight,
                total_validator_weight:_,
                fault_tolerance_fraction:_ } if signature_weight == &Some(Box::new(0.into()))))
        );
    }

    #[test]
    fn should_detect_orphaned_headers() {
        // Chain
        // 0   1   2   3   4   5   6   7   8   9   10   11
        // S           S           S           S
        let switch_blocks = [0, 3, 6, 9];
        let validation_metadata = test_sync_leap_validation_metadata();

        let mut rng = TestRng::new();

        let query = 5;
        let trusted_ancestor_headers = [4, 3];
        let signed_block_headers = [6, 9, 11];
        let add_proofs = true;
        let mut sync_leap = make_test_sync_leap(
            &mut rng,
            &switch_blocks,
            query,
            &trusted_ancestor_headers,
            &signed_block_headers,
            add_proofs,
        );

        // Add single orphaned block. Signatures are cloned from a legit block to avoid bailing on
        // the signature validation check.
        let orphaned_block = Block::random(&mut rng);
        let orphaned_block_with_metadata = BlockHeaderWithMetadata {
            block_header: orphaned_block.header().clone(),
            block_signatures: sync_leap
                .signed_block_headers
                .iter()
                .next()
                .unwrap()
                .block_signatures
                .clone(),
        };
        sync_leap
            .signed_block_headers
            .push(orphaned_block_with_metadata);

        let result = sync_leap.validate(&validation_metadata);
        assert!(matches!(
            result,
            Err(SyncLeapValidationError::IncompleteProof)
        ));
    }

    #[test]
    fn should_detect_orphaned_signatures() {
        const NON_EXISTING_ERA: u64 = u64::MAX;

        // Chain
        // 0   1   2   3   4   5   6   7   8   9   10   11
        // S           S           S           S
        let switch_blocks = [0, 3, 6, 9];
        let validation_metadata = test_sync_leap_validation_metadata();

        let mut rng = TestRng::new();

        let query = 5;
        let trusted_ancestor_headers = [4, 3];
        let signed_block_headers = [6, 9, 11];
        let add_proofs = true;
        let mut sync_leap = make_test_sync_leap(
            &mut rng,
            &switch_blocks,
            query,
            &trusted_ancestor_headers,
            &signed_block_headers,
            add_proofs,
        );

        // Insert signature from an era nowhere near the sync leap data. Base it on one of the
        // existing signatures to avoid bailing on the signature validation check.
        let mut signed_block_header = sync_leap.signed_block_headers.first_mut().unwrap().clone();
        signed_block_header.block_signatures.era_id = NON_EXISTING_ERA.into();
        sync_leap
            .signed_block_headers
            .push(signed_block_header.clone());

        let result = sync_leap.validate(&validation_metadata);
        assert!(matches!(
            result,
            Err(SyncLeapValidationError::IncompleteProof)
        ));
    }

    #[test]
    fn should_fail_when_signature_fails_crypto_verification() {
        // Chain
        // 0   1   2   3   4   5   6   7   8   9   10   11
        // S           S           S           S
        let switch_blocks = [0, 3, 6, 9];
        let validation_metadata = test_sync_leap_validation_metadata();

        let mut rng = TestRng::new();

        let query = 5;
        let trusted_ancestor_headers = [4, 3];
        let signed_block_headers = [6, 9, 11];
        let add_proofs = true;
        let mut sync_leap = make_test_sync_leap(
            &mut rng,
            &switch_blocks,
            query,
            &trusted_ancestor_headers,
            &signed_block_headers,
            add_proofs,
        );

        let mut signed_block_header = sync_leap.signed_block_headers.pop().unwrap();

        // Remove one correct proof.
        let proof = signed_block_header
            .block_signatures
            .proofs
            .pop_last()
            .unwrap();
        let validator_public_key = proof.0;

        // Create unverifiable signature (`Signature::System`).
        let finality_signature = FinalitySignature::new(
            signed_block_header.block_header.block_hash(),
            signed_block_header.block_header.era_id(),
            Signature::System,
            validator_public_key.clone(),
        );

        // Sneak it into the sync leap.
        signed_block_header
            .block_signatures
            .proofs
            .insert(validator_public_key, finality_signature.signature);
        sync_leap.signed_block_headers.push(signed_block_header);

        let result = sync_leap.validate(&validation_metadata);
        assert!(matches!(result, Err(SyncLeapValidationError::Crypto(_))));
    }

    #[test]
    fn should_use_correct_validator_weights_on_upgrade() {
        // Chain
        // 0   1   2   3   4   5   6   7   8   9   10   11
        // S           S           S           S
        let switch_blocks = [0, 3, 6, 9];

        let mut rng = TestRng::new();

        let query = 5;
        let trusted_ancestor_headers = [4, 3];

        const INDEX_OF_THE_LAST_SWITCH_BLOCK: usize = 1;
        let signed_block_headers = [6, 9, 11];

        let add_proofs = true;
        let sync_leap = make_test_sync_leap(
            &mut rng,
            &switch_blocks,
            query,
            &trusted_ancestor_headers,
            &signed_block_headers,
            add_proofs,
        );

        // Setup upgrade after the last switch block.
        let upgrade_block = sync_leap
            .signed_block_headers
            .get(INDEX_OF_THE_LAST_SWITCH_BLOCK)
            .unwrap();
        let upgrade_era = upgrade_block.block_header.era_id().successor();
        let activation_point = ActivationPoint::EraId(upgrade_era);

        // Set up validator change.
        let new_validators: BTreeMap<_, _> =
            iter::repeat_with(|| crypto::generate_ed25519_keypair())
                .take(2)
                .map(|(_, public_key)| (public_key.clone(), 10.into())) // TODO[RC]: No magic numbers
                .collect();
        let global_state_update = GlobalStateUpdate {
            validators: Some(new_validators.clone()),
            entries: Default::default(),
        };

        let unbonding_delay = 7;
        let auction_delay = 1;
        let finality_threshold_fraction = Ratio::new(1, 3);
        let validation_metadata = SyncLeapValidationMetaData::new(
            unbonding_delay - auction_delay, // As per `CoreConfig::recent_era_count()`.
            activation_point,
            Some(global_state_update),
            finality_threshold_fraction,
        );

        let result = sync_leap.validate(&validation_metadata);

        // By asserting on the `HeadersNotSufficientlySigned` error (with bogus validators set to
        // the original validators from the chain) we can prove that the validators smuggled in the
        // validation metadata were actually used in the verification process.
        let expected_bogus_validators: Vec<_> = sync_leap
            .signed_block_headers
            .last()
            .unwrap()
            .block_signatures
            .proofs
            .iter()
            .map(|(public_key, _)| public_key.clone())
            .collect();
        assert!(
            matches!(result, Err(SyncLeapValidationError::HeadersNotSufficientlySigned(inner))
             if matches!(&inner, BlockSignatureError::BogusValidators{
                trusted_validator_weights: _,
                block_signatures: _,
                bogus_validators
            } if bogus_validators == &expected_bogus_validators))
        );
    }

    #[test]
    fn should_return_headers() {
        let mut rng = TestRng::new();

        let trusted_block = Block::random_non_switch_block(&mut rng);

        let trusted_ancestor_1 = Block::random_switch_block(&mut rng);
        let trusted_ancestor_2 = Block::random_non_switch_block(&mut rng);
        let trusted_ancestor_3 = Block::random_non_switch_block(&mut rng);

        let signed_block_1 = Block::random_switch_block(&mut rng);
        let signed_block_2 = Block::random_switch_block(&mut rng);
        let signed_block_3 = Block::random_non_switch_block(&mut rng);
        let signed_block_header_with_metadata_1 =
            make_signed_block_header_from_header(signed_block_1.header(), &[], false);
        let signed_block_header_with_metadata_2 =
            make_signed_block_header_from_header(signed_block_2.header(), &[], false);
        let signed_block_header_with_metadata_3 =
            make_signed_block_header_from_header(signed_block_3.header(), &[], false);

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: trusted_block.header().clone(),
            trusted_ancestor_headers: vec![
                trusted_ancestor_1.header().clone(),
                trusted_ancestor_2.header().clone(),
                trusted_ancestor_3.header().clone(),
            ],
            signed_block_headers: vec![
                signed_block_header_with_metadata_1,
                signed_block_header_with_metadata_2,
                signed_block_header_with_metadata_3,
            ],
        };

        let actual_headers: BTreeSet<_> = sync_leap
            .headers()
            .map(|header| header.block_hash())
            .collect();
        let expected_headers: BTreeSet<_> = [
            trusted_block,
            trusted_ancestor_1,
            trusted_ancestor_2,
            trusted_ancestor_3,
            signed_block_1,
            signed_block_2,
            signed_block_3,
        ]
        .iter()
        .map(|block| block.hash().clone())
        .collect();
        assert_eq!(expected_headers, actual_headers);
    }

    #[test]
    fn should_return_switch_block_headers() {
        let mut rng = TestRng::new();

        let trusted_block = Block::random_non_switch_block(&mut rng);

        let trusted_ancestor_1 = Block::random_switch_block(&mut rng);
        let trusted_ancestor_2 = Block::random_non_switch_block(&mut rng);
        let trusted_ancestor_3 = Block::random_non_switch_block(&mut rng);

        let signed_block_1 = Block::random_switch_block(&mut rng);
        let signed_block_2 = Block::random_switch_block(&mut rng);
        let signed_block_3 = Block::random_non_switch_block(&mut rng);
        let signed_block_header_with_metadata_1 =
            make_signed_block_header_from_header(signed_block_1.header(), &[], false);
        let signed_block_header_with_metadata_2 =
            make_signed_block_header_from_header(signed_block_2.header(), &[], false);
        let signed_block_header_with_metadata_3 =
            make_signed_block_header_from_header(signed_block_3.header(), &[], false);

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: trusted_block.header().clone(),
            trusted_ancestor_headers: vec![
                trusted_ancestor_1.header().clone(),
                trusted_ancestor_2.header().clone(),
                trusted_ancestor_3.header().clone(),
            ],
            signed_block_headers: vec![
                signed_block_header_with_metadata_1.clone(),
                signed_block_header_with_metadata_2.clone(),
                signed_block_header_with_metadata_3.clone(),
            ],
        };

        let actual_headers: BTreeSet<_> = sync_leap
            .switch_blocks_headers()
            .map(|header| header.block_hash())
            .collect();
        let expected_headers: BTreeSet<_> = [
            trusted_ancestor_1.clone(),
            signed_block_1.clone(),
            signed_block_2.clone(),
        ]
        .iter()
        .map(|block| block.hash().clone())
        .collect();
        assert_eq!(expected_headers, actual_headers);

        // Also test when the trusted block is a switch block.
        let trusted_block = Block::random_switch_block(&mut rng);
        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: trusted_block.header().clone(),
            trusted_ancestor_headers: vec![
                trusted_ancestor_1.header().clone(),
                trusted_ancestor_2.header().clone(),
                trusted_ancestor_3.header().clone(),
            ],
            signed_block_headers: vec![
                signed_block_header_with_metadata_1,
                signed_block_header_with_metadata_2,
                signed_block_header_with_metadata_3,
            ],
        };
        let actual_headers: BTreeSet<_> = sync_leap
            .switch_blocks_headers()
            .map(|header| header.block_hash())
            .collect();
        let expected_headers: BTreeSet<_> = [
            trusted_block,
            trusted_ancestor_1,
            signed_block_1,
            signed_block_2,
        ]
        .iter()
        .map(|block| block.hash().clone())
        .collect();
        assert_eq!(expected_headers, actual_headers);
    }
}
