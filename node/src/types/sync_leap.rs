use std::{
    collections::{BTreeMap, HashMap},
    fmt::{self, Display, Formatter},
    iter,
};

use datasize::DataSize;
use itertools::Itertools;
use num_rational::Ratio;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tracing::error;

use casper_types::{
    crypto, BlockHash, BlockHeader, BlockSignatures, Digest, EraId, ProtocolConfig,
    ProtocolVersion, SignedBlockHeader, SignedBlockHeaderValidationError,
};

use crate::{
    components::fetcher::{FetchItem, Tag},
    types::EraValidatorWeights,
    utils::{self, BlockSignatureError},
};

use super::sync_leap_validation_metadata::SyncLeapValidationMetaData;

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
    SignedBlockHeader(SignedBlockHeaderValidationError),
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

// Additional data for syncing blocks immediately after upgrades
#[derive(Debug, Clone, Copy)]
pub(crate) struct GlobalStatesMetadata {
    // Hash, era ID, global state and protocol version of the block after upgrade
    pub(crate) after_hash: BlockHash,
    pub(crate) after_era_id: EraId,
    pub(crate) after_state_hash: Digest,
    pub(crate) after_protocol_version: ProtocolVersion,
    // Hash, global state and protocol version of the block before upgrade
    pub(crate) before_hash: BlockHash,
    pub(crate) before_state_hash: Digest,
    pub(crate) before_protocol_version: ProtocolVersion,
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
    pub signed_block_headers: Vec<SignedBlockHeader>,
}

impl SyncLeap {
    pub(crate) fn era_validator_weights(
        &self,
        fault_tolerance_fraction: Ratio<u64>,
        protocol_config: &ProtocolConfig,
    ) -> impl Iterator<Item = EraValidatorWeights> + '_ {
        // determine if the validator set has been updated in the
        // current protocol version through an emergency upgrade
        let validators_changed_in_current_protocol = protocol_config
            .global_state_update
            .as_ref()
            .map_or(false, |global_state_update| {
                global_state_update.validators.is_some()
            });
        let current_protocol_version = protocol_config.version;

        let block_protocol_versions: HashMap<_, _> = self
            .headers()
            .map(|hdr| (hdr.height(), hdr.protocol_version()))
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
                    // filter out switch blocks preceding upgrades - we don't want to read the era
                    // validators directly from them, as they might have been altered by the
                    // upgrade, we'll get them from the blocks' global states instead
                    //
                    // we can reliably determine if the validator set was changed by an upgrade to
                    // the current protocol version by looking at the chainspec. If validators have
                    // not been altered in any way, then we can use the set reported in the sync
                    // leap by the previous switch block and not read the global states
                    .filter(move |block_header| {
                        block_protocol_versions
                            .get(&(block_header.height() + 1))
                            .map_or(true, |other_protocol_version| {
                                if block_header.protocol_version() == *other_protocol_version {
                                    true
                                } else if *other_protocol_version == current_protocol_version {
                                    !validators_changed_in_current_protocol
                                } else {
                                    false
                                }
                            })
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

    pub(crate) fn global_states_for_sync_across_upgrade(&self) -> Option<GlobalStatesMetadata> {
        let headers_by_height: HashMap<_, _> =
            self.headers().map(|hdr| (hdr.height(), hdr)).collect();

        let maybe_header_before_upgrade = self.switch_blocks_headers().find(|header| {
            headers_by_height
                .get(&(header.height() + 1))
                .map_or(false, |other_header| {
                    other_header.protocol_version() != header.protocol_version()
                })
        });

        maybe_header_before_upgrade.map(|before_header| {
            let after_header = headers_by_height
                .get(&(before_header.height() + 1))
                .unwrap(); // safe, because it had to be Some when we checked it above
            GlobalStatesMetadata {
                after_hash: after_header.block_hash(),
                after_era_id: after_header.era_id(),
                after_state_hash: *after_header.state_root_hash(),
                after_protocol_version: after_header.protocol_version(),
                before_hash: before_header.block_hash(),
                before_state_hash: *before_header.state_root_hash(),
                before_protocol_version: before_header.protocol_version(),
            }
        })
    }

    pub(crate) fn highest_block_height(&self) -> u64 {
        self.headers()
            .map(BlockHeader::height)
            .max()
            .unwrap_or_else(|| self.trusted_block_header.height())
    }

    pub(crate) fn highest_block_header_and_signatures(
        &self,
    ) -> (&BlockHeader, Option<&BlockSignatures>) {
        let header = self
            .headers()
            .max_by_key(|header| header.height())
            .unwrap_or(&self.trusted_block_header);
        let signatures = self
            .signed_block_headers
            .iter()
            .find(|signed_block_header| {
                signed_block_header.block_header().height() == header.height()
            })
            .map(|signed_block_header| signed_block_header.block_signatures());
        (header, signatures)
    }

    pub(crate) fn highest_block_hash(&self) -> BlockHash {
        self.highest_block_header_and_signatures().0.block_hash()
    }

    pub(crate) fn headers(&self) -> impl Iterator<Item = &BlockHeader> {
        iter::once(&self.trusted_block_header)
            .chain(&self.trusted_ancestor_headers)
            .chain(self.signed_block_headers.iter().map(|sh| sh.block_header()))
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
                .entry(signed_header.block_signatures().era_id())
                .or_default()
                .push(signed_header.block_signatures());
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
                    if header.next_block_era_id() == validation_metadata.activation_point.era_id() {
                        if let Some(updated_weights) = validation_metadata
                            .global_state_update
                            .as_ref()
                            .and_then(|update| update.validators.as_ref())
                        {
                            validator_weights = updated_weights
                        }
                    }

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
                            headers_with_sufficient_finality.push(*sigs.block_hash());
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
                .is_valid()
                .map_err(SyncLeapValidationError::SignedBlockHeader)?;
        }

        // defer cryptographic verification until last to avoid unnecessary computation
        for signed_header in &self.signed_block_headers {
            signed_header
                .block_signatures()
                .is_verified()
                .map_err(SyncLeapValidationError::Crypto)?;
        }

        Ok(())
    }
}

mod specimen_support {
    use crate::utils::specimen::{
        estimator_max_rounds_per_era, vec_of_largest_specimen, vec_prop_specimen,
        BlockHeaderWithoutEraEnd, Cache, LargestSpecimen, SizeEstimator,
    };

    use super::{SyncLeap, SyncLeapIdentifier};

    impl LargestSpecimen for SyncLeap {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            // Will at most contain as many blocks as a single era. And how many blocks can
            // there be in an era is determined by the chainspec: it's the
            // maximum of minimum_era_height and era_duration / minimum_block_time
            let count = estimator_max_rounds_per_era(estimator).saturating_sub(1);

            let non_switch_block_ancestors: Vec<BlockHeaderWithoutEraEnd> =
                vec_of_largest_specimen(estimator, count, cache);

            let mut trusted_ancestor_headers =
                vec![LargestSpecimen::largest_specimen(estimator, cache)];
            trusted_ancestor_headers.extend(
                non_switch_block_ancestors
                    .into_iter()
                    .map(BlockHeaderWithoutEraEnd::into_block_header),
            );

            let signed_block_headers = vec_prop_specimen(estimator, "recent_era_count", cache);
            SyncLeap {
                trusted_ancestor_only: LargestSpecimen::largest_specimen(estimator, cache),
                trusted_block_header: LargestSpecimen::largest_specimen(estimator, cache),
                trusted_ancestor_headers,
                signed_block_headers,
            }
        }
    }

    impl LargestSpecimen for SyncLeapIdentifier {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            SyncLeapIdentifier {
                block_hash: LargestSpecimen::largest_specimen(estimator, cache),
                trusted_ancestor_only: true,
            }
        }
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

    use num_rational::Ratio;
    use rand::Rng;

    use casper_types::{
        crypto, testing::TestRng, ActivationPoint, Block, BlockHash, BlockHeader,
        BlockSignaturesV2, BlockV2, ChainNameDigest, EraEndV2, EraId, FinalitySignatureV2,
        GlobalStateUpdate, ProtocolConfig, ProtocolVersion, PublicKey, SecretKey,
        SignedBlockHeader, TestBlockBuilder, Timestamp, TransactionHash, TransactionV1Hash, U512,
    };

    use super::SyncLeap;
    use crate::{
        components::fetcher::FetchItem,
        types::{
            sync_leap::SyncLeapValidationError,
            sync_leap_validation_metadata::SyncLeapValidationMetaData, EraValidatorWeights,
            SyncLeapIdentifier,
        },
        utils::BlockSignatureError,
    };

    fn make_signed_block_header_from_height(
        height: usize,
        test_chain: &[BlockV2],
        validators: &[ValidatorSpec],
        chain_name_hash: ChainNameDigest,
        add_proofs: bool,
    ) -> SignedBlockHeader {
        let header = Block::from(test_chain.get(height).unwrap()).clone_header();
        make_signed_block_header_from_header(&header, validators, chain_name_hash, add_proofs)
    }

    fn make_signed_block_header_from_header(
        block_header: &BlockHeader,
        validators: &[ValidatorSpec],
        chain_name_hash: ChainNameDigest,
        add_proofs: bool,
    ) -> SignedBlockHeader {
        let hash = block_header.block_hash();
        let height = block_header.height();
        let era_id = block_header.era_id();
        let mut block_signatures = BlockSignaturesV2::new(hash, height, era_id, chain_name_hash);
        validators.iter().for_each(
            |ValidatorSpec {
                 secret_key,
                 public_key: _,
                 weight: _,
             }| {
                let fin_sig =
                    FinalitySignatureV2::create(hash, height, era_id, chain_name_hash, secret_key);
                if add_proofs {
                    block_signatures
                        .insert_signature(fin_sig.public_key().clone(), *fin_sig.signature());
                }
            },
        );

        SignedBlockHeader::new(block_header.clone(), block_signatures.into())
    }

    fn make_test_sync_leap_with_chain(
        validators: &[ValidatorSpec],
        test_chain: &[BlockV2],
        query: usize,
        trusted_ancestor_headers: &[usize],
        signed_block_headers: &[usize],
        chain_name_hash: ChainNameDigest,
        add_proofs: bool,
    ) -> SyncLeap {
        let trusted_block_header = Block::from(test_chain.get(query).unwrap()).clone_header();

        let trusted_ancestor_headers: Vec<_> = trusted_ancestor_headers
            .iter()
            .map(|height| Block::from(test_chain.get(*height).unwrap()).clone_header())
            .collect();

        let signed_block_headers: Vec<_> = signed_block_headers
            .iter()
            .map(|height| {
                make_signed_block_header_from_height(
                    *height,
                    test_chain,
                    validators,
                    chain_name_hash,
                    add_proofs,
                )
            })
            .collect();

        SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header,
            trusted_ancestor_headers,
            signed_block_headers,
        }
    }

    // Each generated era gets two validators pulled from the provided `validators` set.
    fn make_test_sync_leap_with_validators(
        rng: &mut TestRng,
        validators: &[ValidatorSpec],
        switch_blocks: &[u64],
        query: usize,
        trusted_ancestor_headers: &[usize],
        signed_block_headers: &[usize],
        add_proofs: bool,
    ) -> SyncLeap {
        let mut test_chain_spec =
            TestChainSpec::new(rng, Some(switch_blocks.to_vec()), None, validators);
        let test_chain: Vec<_> = test_chain_spec.iter().take(12).collect();
        let chain_name_hash = ChainNameDigest::random(rng);

        make_test_sync_leap_with_chain(
            validators,
            &test_chain,
            query,
            trusted_ancestor_headers,
            signed_block_headers,
            chain_name_hash,
            add_proofs,
        )
    }

    fn make_test_sync_leap(
        rng: &mut TestRng,
        switch_blocks: &[u64],
        query: usize,
        trusted_ancestor_headers: &[usize],
        signed_block_headers: &[usize],
        add_proofs: bool,
    ) -> SyncLeap {
        const DEFAULT_VALIDATOR_WEIGHT: u32 = 100;

        let validators: Vec<_> = iter::repeat_with(crypto::generate_ed25519_keypair)
            .take(2)
            .map(|(secret_key, public_key)| ValidatorSpec {
                secret_key,
                public_key,
                weight: Some(DEFAULT_VALIDATOR_WEIGHT.into()),
            })
            .collect();
        make_test_sync_leap_with_validators(
            rng,
            &validators,
            switch_blocks,
            query,
            trusted_ancestor_headers,
            signed_block_headers,
            add_proofs,
        )
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
        let block = TestBlockBuilder::new().height(1).build(&mut rng);

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: block.take_header().into(),
            trusted_ancestor_headers: Default::default(),
            signed_block_headers: Default::default(),
        };
        let result = sync_leap.validate(&validation_metadata);
        assert!(matches!(
            result,
            Err(SyncLeapValidationError::MissingTrustedAncestors)
        ));

        // When trusted block height is 0, validation should not fail due trusted ancestors being
        // empty.
        let block = TestBlockBuilder::new().height(0).build(&mut rng);

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: block.take_header().into(),
            trusted_ancestor_headers: Default::default(),
            signed_block_headers: Default::default(),
        };
        let result = sync_leap.validate(&validation_metadata);
        assert!(!matches!(
            result,
            Err(SyncLeapValidationError::MissingTrustedAncestors)
        ));
        assert!(result.is_ok());
    }

    #[test]
    fn should_check_signed_block_headers_size() {
        let mut rng = TestRng::new();
        let validation_metadata = test_sync_leap_validation_metadata();

        let max_allowed_size = validation_metadata.recent_era_count + 1;

        // Max allowed size should NOT trigger the `TooManySwitchBlocks` error.
        let generated_block_count = max_allowed_size;

        let block = TestBlockBuilder::new().height(0).build_versioned(&mut rng);
        let chain_name_hash = ChainNameDigest::random(&mut rng);
        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: block.clone_header(),
            trusted_ancestor_headers: Default::default(),
            signed_block_headers: iter::repeat_with(|| {
                let block = TestBlockBuilder::new().build_versioned(&mut rng);
                let hash = block.hash();
                let height = block.height();
                SignedBlockHeader::new(
                    block.clone_header(),
                    BlockSignaturesV2::new(*hash, height, 0.into(), chain_name_hash).into(),
                )
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

        let block = TestBlockBuilder::new().height(0).build_versioned(&mut rng);
        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: block.take_header(),
            trusted_ancestor_headers: Default::default(),
            signed_block_headers: iter::repeat_with(|| {
                let block = TestBlockBuilder::new().build_versioned(&mut rng);
                let hash = block.hash();
                let height = block.height();
                SignedBlockHeader::new(
                    block.clone_header(),
                    BlockSignaturesV2::new(*hash, height, 0.into(), chain_name_hash).into(),
                )
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
        let block = TestBlockBuilder::new().height(0).build(&mut rng);
        let block_iterator =
            TestBlockIterator::new(block.clone(), &mut rng, None, None, Default::default());
        let block = Block::from(block);

        let trusted_ancestor_headers = block_iterator
            .take(3)
            .map(|block| block.take_header().into())
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
        let block = TestBlockBuilder::new().height(0).build(&mut rng);
        let block_iterator =
            TestBlockIterator::new(block.clone(), &mut rng, None, None, Default::default());

        let trusted_ancestor_headers = block_iterator
            .take(1)
            .map(|block| block.take_header().into())
            .collect();

        let block = Block::from(block);
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

        let block = TestBlockBuilder::new().height(0).build(&mut rng);
        let block_iterator = TestBlockIterator::new(
            block.clone(),
            &mut rng,
            switch_blocks,
            None,
            Default::default(),
        );

        let trusted_ancestor_headers: Vec<_> = block_iterator
            .take(3)
            .map(|block| block.take_header().into())
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();

        let block = Block::from(block);
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
        let orphaned_block = TestBlockBuilder::new().build_versioned(&mut rng);
        let orphaned_signed_block_header = SignedBlockHeader::new(
            orphaned_block.clone_header(),
            sync_leap
                .signed_block_headers
                .first()
                .unwrap()
                .block_signatures()
                .clone(),
        );
        sync_leap
            .signed_block_headers
            .push(orphaned_signed_block_header);

        let result = sync_leap.validate(&validation_metadata);
        assert!(matches!(
            result,
            Err(SyncLeapValidationError::IncompleteProof)
        ));
    }

    #[test]
    fn should_detect_orphaned_signatures() {
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
        let mut invalid_signed_block_header =
            sync_leap.signed_block_headers.first_mut().unwrap().clone();
        invalid_signed_block_header.invalidate_era();
        sync_leap
            .signed_block_headers
            .push(invalid_signed_block_header);

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

        let mut invalid_signed_block_header = sync_leap.signed_block_headers.pop().unwrap();
        invalid_signed_block_header.invalidate_last_signature();
        sync_leap
            .signed_block_headers
            .push(invalid_signed_block_header);

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
        let upgrade_era = upgrade_block.block_header().era_id().successor();
        let activation_point = ActivationPoint::EraId(upgrade_era);

        // Set up validator change.
        const DEFAULT_VALIDATOR_WEIGHT: u64 = 100;
        let new_validators: BTreeMap<_, _> = iter::repeat_with(crypto::generate_ed25519_keypair)
            .take(2)
            .map(|(_, public_key)| (public_key, DEFAULT_VALIDATOR_WEIGHT.into()))
            .collect();
        let global_state_update = GlobalStateUpdate {
            validators: Some(new_validators),
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
            .block_signatures()
            .signers()
            .cloned()
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
        let chain_name_hash = ChainNameDigest::random(&mut rng);

        let trusted_block = TestBlockBuilder::new()
            .switch_block(false)
            .build_versioned(&mut rng);

        let trusted_ancestor_1 = TestBlockBuilder::new()
            .switch_block(true)
            .build_versioned(&mut rng);
        let trusted_ancestor_2 = TestBlockBuilder::new()
            .switch_block(false)
            .build_versioned(&mut rng);
        let trusted_ancestor_3 = TestBlockBuilder::new()
            .switch_block(false)
            .build_versioned(&mut rng);

        let signed_block_1 = TestBlockBuilder::new()
            .switch_block(true)
            .build_versioned(&mut rng);
        let signed_block_2 = TestBlockBuilder::new()
            .switch_block(true)
            .build_versioned(&mut rng);
        let signed_block_3 = TestBlockBuilder::new()
            .switch_block(false)
            .build_versioned(&mut rng);
        let signed_block_header_1 = make_signed_block_header_from_header(
            &signed_block_1.clone_header(),
            &[],
            chain_name_hash,
            false,
        );
        let signed_block_header_2 = make_signed_block_header_from_header(
            &signed_block_2.clone_header(),
            &[],
            chain_name_hash,
            false,
        );
        let signed_block_header_3 = make_signed_block_header_from_header(
            &signed_block_3.clone_header(),
            &[],
            chain_name_hash,
            false,
        );

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: trusted_block.clone_header(),
            trusted_ancestor_headers: vec![
                trusted_ancestor_1.clone_header(),
                trusted_ancestor_2.clone_header(),
                trusted_ancestor_3.clone_header(),
            ],
            signed_block_headers: vec![
                signed_block_header_1,
                signed_block_header_2,
                signed_block_header_3,
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
        .map(|block| *block.hash())
        .collect();
        assert_eq!(expected_headers, actual_headers);
    }

    #[test]
    fn should_return_switch_block_headers() {
        let mut rng = TestRng::new();

        let chain_name_hash = ChainNameDigest::random(&mut rng);

        let trusted_block = TestBlockBuilder::new()
            .switch_block(false)
            .build_versioned(&mut rng);

        let trusted_ancestor_1 = TestBlockBuilder::new()
            .switch_block(true)
            .build_versioned(&mut rng);
        let trusted_ancestor_2 = TestBlockBuilder::new()
            .switch_block(false)
            .build_versioned(&mut rng);
        let trusted_ancestor_3 = TestBlockBuilder::new()
            .switch_block(false)
            .build_versioned(&mut rng);

        let signed_block_1 = TestBlockBuilder::new()
            .switch_block(true)
            .build_versioned(&mut rng);
        let signed_block_2 = TestBlockBuilder::new()
            .switch_block(true)
            .build_versioned(&mut rng);
        let signed_block_3 = TestBlockBuilder::new()
            .switch_block(false)
            .build_versioned(&mut rng);
        let signed_block_header_1 = make_signed_block_header_from_header(
            &signed_block_1.clone_header(),
            &[],
            chain_name_hash,
            false,
        );
        let signed_block_header_2 = make_signed_block_header_from_header(
            &signed_block_2.clone_header(),
            &[],
            chain_name_hash,
            false,
        );
        let signed_block_header_3 = make_signed_block_header_from_header(
            &signed_block_3.clone_header(),
            &[],
            chain_name_hash,
            false,
        );

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: trusted_block.clone_header(),
            trusted_ancestor_headers: vec![
                trusted_ancestor_1.clone_header(),
                trusted_ancestor_2.clone_header(),
                trusted_ancestor_3.clone_header(),
            ],
            signed_block_headers: vec![
                signed_block_header_1.clone(),
                signed_block_header_2.clone(),
                signed_block_header_3.clone(),
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
        .map(|block| *block.hash())
        .collect();
        assert_eq!(expected_headers, actual_headers);

        // Also test when the trusted block is a switch block.
        let trusted_block = TestBlockBuilder::new()
            .switch_block(true)
            .build_versioned(&mut rng);
        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: trusted_block.clone_header(),
            trusted_ancestor_headers: vec![
                trusted_ancestor_1.clone_header(),
                trusted_ancestor_2.clone_header(),
                trusted_ancestor_3.clone_header(),
            ],
            signed_block_headers: vec![
                signed_block_header_1,
                signed_block_header_2,
                signed_block_header_3,
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
        .map(|block| *block.hash())
        .collect();
        assert_eq!(expected_headers, actual_headers);
    }

    #[test]
    fn should_return_highest_block_header_from_trusted_block() {
        // Chain
        // 0   1   2   3   4   5   6   7   8   9   10   11
        // S           S           S           S
        let switch_blocks = [0, 3, 6, 9];

        let mut rng = TestRng::new();

        let query = 5;
        let trusted_ancestor_headers = [4, 3];
        let signed_block_headers = [6, 9, 11];
        let add_proofs = true;
        let valid_sync_leap = make_test_sync_leap(
            &mut rng,
            &switch_blocks,
            query,
            &trusted_ancestor_headers,
            &signed_block_headers,
            add_proofs,
        );

        // `valid_sync_leap` created above is a well formed SyncLeap structure for the test chain.
        // We can use the blocks it contains to generate SyncLeap structures as required for
        // the test, because we know the heights of the blocks in the test chain as well as
        // their sigs.
        let highest_block = valid_sync_leap
            .signed_block_headers
            .last()
            .unwrap()
            .block_header()
            .clone();
        let lowest_blocks: Vec<_> = valid_sync_leap
            .trusted_ancestor_headers
            .iter()
            .take(2)
            .cloned()
            .collect();
        let middle_blocks: Vec<_> = valid_sync_leap
            .signed_block_headers
            .iter()
            .take(2)
            .cloned()
            .collect();

        let highest_block_height = highest_block.height();
        let highest_block_hash = highest_block.block_hash();

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: highest_block.clone(),
            trusted_ancestor_headers: lowest_blocks,
            signed_block_headers: middle_blocks,
        };
        assert_eq!(
            sync_leap
                .highest_block_header_and_signatures()
                .0
                .block_hash(),
            highest_block.block_hash()
        );
        assert_eq!(sync_leap.highest_block_hash(), highest_block_hash);
        assert_eq!(sync_leap.highest_block_height(), highest_block_height);
    }

    #[test]
    fn should_return_highest_block_header_from_trusted_ancestors() {
        // Chain
        // 0   1   2   3   4   5   6   7   8   9   10   11
        // S           S           S           S
        let switch_blocks = [0, 3, 6, 9];

        let mut rng = TestRng::new();

        let query = 5;
        let trusted_ancestor_headers = [4, 3];
        let signed_block_headers = [6, 9, 11];
        let add_proofs = true;
        let valid_sync_leap = make_test_sync_leap(
            &mut rng,
            &switch_blocks,
            query,
            &trusted_ancestor_headers,
            &signed_block_headers,
            add_proofs,
        );

        // `valid_sync_leap` created above is a well formed SyncLeap structure for the test chain.
        // We can use the blocks it contains to generate SyncLeap structures as required for
        // the test, because we know the heights of the blocks in the test chain as well as
        // their sigs.
        let highest_block = valid_sync_leap
            .signed_block_headers
            .last()
            .unwrap()
            .block_header()
            .clone();
        let lowest_blocks: Vec<_> = valid_sync_leap
            .trusted_ancestor_headers
            .iter()
            .take(2)
            .cloned()
            .collect();
        let middle_blocks: Vec<_> = valid_sync_leap
            .signed_block_headers
            .iter()
            .take(2)
            .cloned()
            .collect();

        let highest_block_height = highest_block.height();
        let highest_block_hash = highest_block.block_hash();

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: lowest_blocks.first().unwrap().clone(),
            trusted_ancestor_headers: vec![highest_block],
            signed_block_headers: middle_blocks,
        };
        assert_eq!(
            sync_leap
                .highest_block_header_and_signatures()
                .0
                .block_hash(),
            highest_block_hash
        );
        assert_eq!(sync_leap.highest_block_hash(), highest_block_hash);
        assert_eq!(sync_leap.highest_block_height(), highest_block_height);
    }

    #[test]
    fn should_return_highest_block_header_from_signed_block_headers() {
        // Chain
        // 0   1   2   3   4   5   6   7   8   9   10   11
        // S           S           S           S
        let switch_blocks = [0, 3, 6, 9];

        let mut rng = TestRng::new();

        let query = 5;
        let trusted_ancestor_headers = [4, 3];
        let signed_block_headers = [6, 9, 11];
        let add_proofs = true;
        let valid_sync_leap = make_test_sync_leap(
            &mut rng,
            &switch_blocks,
            query,
            &trusted_ancestor_headers,
            &signed_block_headers,
            add_proofs,
        );

        // `valid_sync_leap` created above is a well formed SyncLeap structure for the test chain.
        // We can use the blocks it contains to generate SyncLeap structures as required for
        // the test, because we know the heights of the blocks in the test chain as well as
        // their sigs.
        let highest_block = valid_sync_leap.signed_block_headers.last().unwrap().clone();
        let lowest_blocks: Vec<_> = valid_sync_leap
            .trusted_ancestor_headers
            .iter()
            .take(2)
            .cloned()
            .collect();
        let middle_blocks: Vec<_> = valid_sync_leap
            .signed_block_headers
            .iter()
            .take(2)
            .cloned()
            .map(|signed_block_header| signed_block_header.block_header().clone())
            .collect();

        let highest_block_height = highest_block.block_header().height();
        let highest_block_hash = highest_block.block_header().block_hash();

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: lowest_blocks.first().unwrap().clone(),
            trusted_ancestor_headers: middle_blocks,
            signed_block_headers: vec![highest_block.clone()],
        };
        assert_eq!(
            sync_leap
                .highest_block_header_and_signatures()
                .0
                .block_hash(),
            highest_block.block_header().block_hash()
        );
        assert_eq!(sync_leap.highest_block_hash(), highest_block_hash);
        assert_eq!(sync_leap.highest_block_height(), highest_block_height);
    }

    #[test]
    fn should_return_sigs_when_highest_block_is_signed() {
        // Chain
        // 0   1   2   3   4   5   6   7   8   9   10   11
        // S           S           S           S
        let switch_blocks = [0, 3, 6, 9];

        let mut rng = TestRng::new();

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

        assert!(sync_leap.highest_block_header_and_signatures().1.is_some());
    }

    #[test]
    fn should_not_return_sigs_when_highest_block_is_not_signed() {
        // Chain
        // 0   1   2   3   4   5   6   7   8   9   10   11
        // S           S           S           S
        let switch_blocks = [0, 3, 6, 9];

        let mut rng = TestRng::new();

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

        // `sync_leap` is a well formed SyncLeap structure for the test chain. We can use the blocks
        // it contains to generate SyncLeap structures as required for the test, because we know the
        // heights of the blocks in the test chain as well as their sigs.
        let highest_block = sync_leap.signed_block_headers.last().unwrap().clone();
        let lowest_blocks: Vec<_> = sync_leap
            .trusted_ancestor_headers
            .iter()
            .take(2)
            .cloned()
            .collect();
        let middle_blocks: Vec<_> = sync_leap
            .signed_block_headers
            .iter()
            .take(2)
            .cloned()
            .collect();
        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: highest_block.block_header().clone(),
            trusted_ancestor_headers: lowest_blocks,
            signed_block_headers: middle_blocks,
        };
        assert!(sync_leap.highest_block_header_and_signatures().1.is_none());
    }

    #[test]
    fn should_return_era_validator_weights_for_correct_sync_leap() {
        // Chain
        // 0   1   2   3   4   5   6   7   8   9   10   11
        // S           S           S           S
        let switch_blocks = [0, 3, 6, 9];

        let mut rng = TestRng::new();

        // Test block iterator will pull 2 validators for each created block. Indices 0 and 1 are
        // used for validators for the trusted ancestor headers.
        const FIRST_SIGNED_BLOCK_HEADER_VALIDATOR_OFFSET: usize = 2;

        let validators: Vec<_> = (1..100)
            .map(|weight| {
                let (secret_key, public_key) = crypto::generate_ed25519_keypair();
                ValidatorSpec {
                    secret_key,
                    public_key,
                    weight: Some(U512::from(weight)),
                }
            })
            .collect();

        let query = 5;
        let trusted_ancestor_headers = [4, 3];
        let signed_block_headers = [6, 9, 11];
        let add_proofs = true;
        let sync_leap = make_test_sync_leap_with_validators(
            &mut rng,
            &validators,
            &switch_blocks,
            query,
            &trusted_ancestor_headers,
            &signed_block_headers,
            add_proofs,
        );

        let fault_tolerance_fraction = Ratio::new_raw(1, 3);

        let mut block_iter = sync_leap.signed_block_headers.iter();
        let first_switch_block = block_iter.next().unwrap().clone();
        let protocol_version = first_switch_block.block_header().protocol_version();
        let validator_1 = validators
            .get(FIRST_SIGNED_BLOCK_HEADER_VALIDATOR_OFFSET)
            .unwrap();
        let validator_2 = validators
            .get(FIRST_SIGNED_BLOCK_HEADER_VALIDATOR_OFFSET + 1)
            .unwrap();
        let first_era_validator_weights = EraValidatorWeights::new(
            first_switch_block.block_header().era_id(),
            [validator_1, validator_2]
                .iter()
                .map(
                    |ValidatorSpec {
                         secret_key: _,
                         public_key,
                         weight,
                     }| (public_key.clone(), weight.unwrap()),
                )
                .collect(),
            fault_tolerance_fraction,
        );

        let second_switch_block = block_iter.next().unwrap().clone();
        let validator_1 = validators
            .get(FIRST_SIGNED_BLOCK_HEADER_VALIDATOR_OFFSET + 2)
            .unwrap();
        let validator_2 = validators
            .get(FIRST_SIGNED_BLOCK_HEADER_VALIDATOR_OFFSET + 3)
            .unwrap();
        let second_era_validator_weights = EraValidatorWeights::new(
            second_switch_block.block_header().era_id(),
            [validator_1, validator_2]
                .iter()
                .map(
                    |ValidatorSpec {
                         secret_key: _,
                         public_key,
                         weight,
                     }| (public_key.clone(), weight.unwrap()),
                )
                .collect(),
            fault_tolerance_fraction,
        );

        let third_block = block_iter.next().unwrap().clone();
        let validator_1 = validators
            .get(FIRST_SIGNED_BLOCK_HEADER_VALIDATOR_OFFSET + 4)
            .unwrap();
        let validator_2 = validators
            .get(FIRST_SIGNED_BLOCK_HEADER_VALIDATOR_OFFSET + 5)
            .unwrap();
        let third_era_validator_weights = EraValidatorWeights::new(
            third_block.block_header().era_id(),
            [validator_1, validator_2]
                .iter()
                .map(
                    |ValidatorSpec {
                         secret_key: _,
                         public_key,
                         weight,
                     }| (public_key.clone(), weight.unwrap()),
                )
                .collect(),
            fault_tolerance_fraction,
        );

        let protocol_config = ProtocolConfig {
            version: protocol_version,
            global_state_update: None,
            activation_point: ActivationPoint::EraId(EraId::random(&mut rng)),
            hard_reset: rng.gen(),
        };

        let result: Vec<_> = sync_leap
            .era_validator_weights(fault_tolerance_fraction, &protocol_config)
            .collect();
        assert_eq!(
            result,
            vec![
                first_era_validator_weights,
                second_era_validator_weights,
                third_era_validator_weights
            ]
        )
    }

    #[test]
    fn should_not_return_global_states_when_no_upgrade() {
        let mut rng = TestRng::new();

        const DEFAULT_VALIDATOR_WEIGHT: u32 = 100;

        let chain_name_hash = ChainNameDigest::random(&mut rng);

        let validators: Vec<_> = iter::repeat_with(crypto::generate_ed25519_keypair)
            .take(2)
            .map(|(secret_key, public_key)| ValidatorSpec {
                secret_key,
                public_key,
                weight: Some(DEFAULT_VALIDATOR_WEIGHT.into()),
            })
            .collect();

        let mut test_chain_spec = TestChainSpec::new(&mut rng, Some(vec![4, 8]), None, &validators);
        let chain: Vec<_> = test_chain_spec.iter().take(12).collect();

        let sync_leap = make_test_sync_leap_with_chain(
            &validators,
            &chain,
            11,
            &[10, 9, 8],
            &[],
            chain_name_hash,
            false,
        );

        let global_states_metadata = sync_leap.global_states_for_sync_across_upgrade();
        assert!(global_states_metadata.is_none());
    }

    #[test]
    fn should_return_global_states_when_upgrade() {
        let mut rng = TestRng::new();

        const DEFAULT_VALIDATOR_WEIGHT: u32 = 100;

        let chain_name_hash = ChainNameDigest::random(&mut rng);

        let validators: Vec<_> = iter::repeat_with(crypto::generate_ed25519_keypair)
            .take(2)
            .map(|(secret_key, public_key)| ValidatorSpec {
                secret_key,
                public_key,
                weight: Some(DEFAULT_VALIDATOR_WEIGHT.into()),
            })
            .collect();

        let mut test_chain_spec =
            TestChainSpec::new(&mut rng, Some(vec![4, 8]), Some(vec![8]), &validators);
        let chain: Vec<_> = test_chain_spec.iter().take(12).collect();

        let sync_leap = make_test_sync_leap_with_chain(
            &validators,
            &chain,
            11,
            &[10, 9, 8],
            &[],
            chain_name_hash,
            false,
        );

        let global_states_metadata = sync_leap
            .global_states_for_sync_across_upgrade()
            .expect("should be Some");

        assert_eq!(global_states_metadata.after_hash, *chain[9].hash());
        assert_eq!(global_states_metadata.after_era_id, chain[9].era_id());
        assert_eq!(
            global_states_metadata.after_protocol_version,
            chain[9].protocol_version()
        );
        assert_eq!(
            global_states_metadata.after_state_hash,
            *chain[9].state_root_hash()
        );

        assert_eq!(global_states_metadata.before_hash, *chain[8].hash());
        assert_eq!(
            global_states_metadata.before_protocol_version,
            chain[8].protocol_version()
        );
        assert_eq!(
            global_states_metadata.before_state_hash,
            *chain[8].state_root_hash()
        );

        assert_ne!(
            global_states_metadata.before_protocol_version,
            global_states_metadata.after_protocol_version
        );
    }

    #[test]
    fn should_return_global_states_when_immediate_switch_block() {
        let mut rng = TestRng::new();

        const DEFAULT_VALIDATOR_WEIGHT: u32 = 100;

        let chain_name_hash = ChainNameDigest::random(&mut rng);

        let validators: Vec<_> = iter::repeat_with(crypto::generate_ed25519_keypair)
            .take(2)
            .map(|(secret_key, public_key)| ValidatorSpec {
                secret_key,
                public_key,
                weight: Some(DEFAULT_VALIDATOR_WEIGHT.into()),
            })
            .collect();

        let mut test_chain_spec =
            TestChainSpec::new(&mut rng, Some(vec![4, 8, 9]), Some(vec![8]), &validators);
        let chain: Vec<_> = test_chain_spec.iter().take(12).collect();

        let sync_leap = make_test_sync_leap_with_chain(
            &validators,
            &chain,
            9,
            &[8],
            &[],
            chain_name_hash,
            false,
        );

        let global_states_metadata = sync_leap
            .global_states_for_sync_across_upgrade()
            .expect("should be Some");

        assert_eq!(global_states_metadata.after_hash, *chain[9].hash());
        assert_eq!(global_states_metadata.after_era_id, chain[9].era_id());
        assert_eq!(
            global_states_metadata.after_protocol_version,
            chain[9].protocol_version()
        );
        assert_eq!(
            global_states_metadata.after_state_hash,
            *chain[9].state_root_hash()
        );

        assert_eq!(global_states_metadata.before_hash, *chain[8].hash());
        assert_eq!(
            global_states_metadata.before_protocol_version,
            chain[8].protocol_version()
        );
        assert_eq!(
            global_states_metadata.before_state_hash,
            *chain[8].state_root_hash()
        );

        assert_ne!(
            global_states_metadata.before_protocol_version,
            global_states_metadata.after_protocol_version
        );
    }

    #[test]
    fn era_validator_weights_without_genesis_without_upgrade() {
        let mut rng = TestRng::new();

        let trusted_block = TestBlockBuilder::new()
            .switch_block(false)
            .build_versioned(&mut rng);

        let version = ProtocolVersion::from_parts(1, 5, 0);

        let (signed_block_header_1, signed_block_header_2, signed_block_header_3) =
            make_three_switch_blocks_at_era_and_height_and_version(
                &mut rng,
                (1, 10, version),
                (2, 20, version),
                (3, 30, version),
            );

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: trusted_block.clone_header(),
            trusted_ancestor_headers: vec![],
            signed_block_headers: vec![
                signed_block_header_1,
                signed_block_header_2,
                signed_block_header_3,
            ],
        };

        let fault_tolerance_fraction = Ratio::new_raw(1, 3);

        // Assert only if correct eras are selected, since the
        // `should_return_era_validator_weights_for_correct_sync_leap` test already covers the
        // actual weight validation.

        let protocol_config = ProtocolConfig {
            version,
            global_state_update: None,
            hard_reset: false,
            activation_point: ActivationPoint::EraId(EraId::random(&mut rng)),
        };

        let actual_eras: BTreeSet<u64> = sync_leap
            .era_validator_weights(fault_tolerance_fraction, &protocol_config)
            .map(|era_validator_weights| era_validator_weights.era_id().into())
            .collect();
        let mut expected_eras: BTreeSet<u64> = BTreeSet::new();
        // Expect successors of the eras of switch blocks.
        expected_eras.extend([2, 3, 4]);
        assert_eq!(expected_eras, actual_eras);
    }

    #[test]
    fn era_validator_weights_without_genesis_with_switch_block_preceding_immediate_switch_block() {
        let mut rng = TestRng::new();

        let trusted_block = TestBlockBuilder::new()
            .switch_block(false)
            .build_versioned(&mut rng);

        let version_1 = ProtocolVersion::from_parts(1, 4, 0);
        let version_2 = ProtocolVersion::from_parts(1, 5, 0);

        let (signed_block_header_1, signed_block_header_2, signed_block_header_3) =
            make_three_switch_blocks_at_era_and_height_and_version(
                &mut rng,
                (1, 10, version_1),
                (2, 20, version_1),
                (3, 21, version_2),
            );

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: trusted_block.clone_header(),
            trusted_ancestor_headers: vec![],
            signed_block_headers: vec![
                signed_block_header_1,
                signed_block_header_2,
                signed_block_header_3,
            ],
        };

        let fault_tolerance_fraction = Ratio::new_raw(1, 3);

        // Assert only if correct eras are selected, since the
        // `should_return_era_validator_weights_for_correct_sync_leap` test already covers the
        // actual weight validation.

        let protocol_config = ProtocolConfig {
            version: version_2,
            global_state_update: Some(GlobalStateUpdate {
                validators: Some(BTreeMap::new()),
                entries: BTreeMap::new(),
            }),
            hard_reset: false,
            activation_point: ActivationPoint::EraId(EraId::random(&mut rng)),
        };

        let actual_eras: BTreeSet<u64> = sync_leap
            .era_validator_weights(fault_tolerance_fraction, &protocol_config)
            .map(|era_validator_weights| era_validator_weights.era_id().into())
            .collect();
        let mut expected_eras: BTreeSet<u64> = BTreeSet::new();

        // Block #1 (era=1, height=10)
        // Block #2 (era=2, height=20) - block preceding immediate switch block
        // Block #3 (era=3, height=21) - immediate switch block.
        // Expect the successor of block #2 to be not present.
        expected_eras.extend([2, 4]);
        assert_eq!(expected_eras, actual_eras);

        let protocol_config = ProtocolConfig {
            version: version_2,
            global_state_update: None,
            hard_reset: rng.gen(),
            activation_point: ActivationPoint::EraId(EraId::random(&mut rng)),
        };

        let actual_eras: BTreeSet<u64> = sync_leap
            .era_validator_weights(fault_tolerance_fraction, &protocol_config)
            .map(|era_validator_weights| era_validator_weights.era_id().into())
            .collect();
        let mut expected_eras: BTreeSet<u64> = BTreeSet::new();

        // Block #1 (era=1, height=10)
        // Block #2 (era=2, height=20) - block preceding immediate switch block
        // Block #3 (era=3, height=21) - immediate switch block.
        // Expect era 3 to be present since the upgrade did not change the validators in any way.
        expected_eras.extend([2, 3, 4]);
        assert_eq!(expected_eras, actual_eras);
    }

    #[test]
    fn era_validator_weights_with_genesis_without_upgrade() {
        let mut rng = TestRng::new();

        let trusted_block = TestBlockBuilder::new()
            .switch_block(false)
            .build_versioned(&mut rng);

        let version = ProtocolVersion::from_parts(1, 5, 0);

        let (signed_block_header_1, signed_block_header_2, signed_block_header_3) =
            make_three_switch_blocks_at_era_and_height_and_version(
                &mut rng,
                (0, 0, version),
                (1, 10, version),
                (2, 20, version),
            );

        let sync_leap = SyncLeap {
            trusted_ancestor_only: false,
            trusted_block_header: trusted_block.clone_header(),
            trusted_ancestor_headers: vec![],
            signed_block_headers: vec![
                signed_block_header_1,
                signed_block_header_2,
                signed_block_header_3,
            ],
        };

        let fault_tolerance_fraction = Ratio::new_raw(1, 3);

        // Assert only if correct eras are selected, since the the
        // `should_return_era_validator_weights_for_correct_sync_leap` test already covers the
        // actual weight validation.
        let protocol_config = ProtocolConfig {
            version,
            global_state_update: None,
            hard_reset: false,
            activation_point: ActivationPoint::EraId(EraId::random(&mut rng)),
        };

        let actual_eras: BTreeSet<u64> = sync_leap
            .era_validator_weights(fault_tolerance_fraction, &protocol_config)
            .map(|era_validator_weights| era_validator_weights.era_id().into())
            .collect();
        let mut expected_eras: BTreeSet<u64> = BTreeSet::new();
        // Expect genesis era id and its successor as well as the successors of the eras of
        // non-genesis switch blocks.
        expected_eras.extend([0, 1, 2, 3]);
        assert_eq!(expected_eras, actual_eras);
    }

    fn make_three_switch_blocks_at_era_and_height_and_version(
        rng: &mut TestRng,
        (era_1, height_1, version_1): (u64, u64, ProtocolVersion),
        (era_2, height_2, version_2): (u64, u64, ProtocolVersion),
        (era_3, height_3, version_3): (u64, u64, ProtocolVersion),
    ) -> (SignedBlockHeader, SignedBlockHeader, SignedBlockHeader) {
        let chain_name_hash = ChainNameDigest::random(rng);
        let signed_block_1 = TestBlockBuilder::new()
            .height(height_1)
            .era(era_1)
            .protocol_version(version_1)
            .switch_block(true)
            .build_versioned(rng);
        let signed_block_2 = TestBlockBuilder::new()
            .height(height_2)
            .era(era_2)
            .protocol_version(version_2)
            .switch_block(true)
            .build_versioned(rng);
        let signed_block_3 = TestBlockBuilder::new()
            .height(height_3)
            .era(era_3)
            .protocol_version(version_3)
            .switch_block(true)
            .build_versioned(rng);

        let signed_block_header_1 = make_signed_block_header_from_header(
            &signed_block_1.clone_header(),
            &[],
            chain_name_hash,
            false,
        );
        let signed_block_header_2 = make_signed_block_header_from_header(
            &signed_block_2.clone_header(),
            &[],
            chain_name_hash,
            false,
        );
        let signed_block_header_3 = make_signed_block_header_from_header(
            &signed_block_3.clone_header(),
            &[],
            chain_name_hash,
            false,
        );
        (
            signed_block_header_1,
            signed_block_header_2,
            signed_block_header_3,
        )
    }

    #[test]
    fn should_construct_proper_sync_leap_identifier() {
        let mut rng = TestRng::new();

        let sync_leap_identifier = SyncLeapIdentifier::sync_to_tip(BlockHash::random(&mut rng));
        assert!(!sync_leap_identifier.trusted_ancestor_only());

        let sync_leap_identifier =
            SyncLeapIdentifier::sync_to_historical(BlockHash::random(&mut rng));
        assert!(sync_leap_identifier.trusted_ancestor_only());
    }

    // Describes a single item from the set of validators that will be used for switch blocks
    // created by TestChainSpec.
    pub(crate) struct ValidatorSpec {
        pub(crate) secret_key: SecretKey,
        pub(crate) public_key: PublicKey,
        // If `None`, weight will be chosen randomly.
        pub(crate) weight: Option<U512>,
    }

    // Utility struct that can be turned into an iterator that generates
    // continuous and descending blocks (i.e. blocks that have consecutive height
    // and parent hashes are correctly set). The height of the first block
    // in a series is chosen randomly.
    //
    // Additionally, this struct allows to generate switch blocks at a specific location in the
    // chain, for example: Setting `switch_block_indices` to [1, 3] and generating 5 blocks will
    // cause the 2nd and 4th blocks to be switch blocks. Validators for all eras are filled from
    // the `validators` parameter.
    pub(crate) struct TestChainSpec<'a> {
        block: BlockV2,
        rng: &'a mut TestRng,
        switch_block_indices: Option<Vec<u64>>,
        upgrades_indices: Option<Vec<u64>>,
        validators: &'a [ValidatorSpec],
    }

    impl<'a> TestChainSpec<'a> {
        pub(crate) fn new(
            test_rng: &'a mut TestRng,
            switch_block_indices: Option<Vec<u64>>,
            upgrades_indices: Option<Vec<u64>>,
            validators: &'a [ValidatorSpec],
        ) -> Self {
            let block = TestBlockBuilder::new().build(test_rng);
            Self {
                block,
                rng: test_rng,
                switch_block_indices,
                upgrades_indices,
                validators,
            }
        }

        pub(crate) fn iter(&mut self) -> TestBlockIterator {
            let block_height = self.block.height();

            const DEFAULT_VALIDATOR_WEIGHT: u64 = 100;

            TestBlockIterator::new(
                self.block.clone(),
                self.rng,
                self.switch_block_indices
                    .clone()
                    .map(|switch_block_indices| {
                        switch_block_indices
                            .iter()
                            .map(|index| index + block_height)
                            .collect()
                    }),
                self.upgrades_indices.clone().map(|upgrades_indices| {
                    upgrades_indices
                        .iter()
                        .map(|index| index + block_height)
                        .collect()
                }),
                self.validators
                    .iter()
                    .map(
                        |ValidatorSpec {
                             secret_key: _,
                             public_key,
                             weight,
                         }| {
                            (
                                public_key.clone(),
                                weight.unwrap_or(DEFAULT_VALIDATOR_WEIGHT.into()),
                            )
                        },
                    )
                    .collect(),
            )
        }
    }

    pub(crate) struct TestBlockIterator<'a> {
        block: BlockV2,
        protocol_version: ProtocolVersion,
        rng: &'a mut TestRng,
        switch_block_indices: Option<Vec<u64>>,
        upgrades_indices: Option<Vec<u64>>,
        validators: Vec<(PublicKey, U512)>,
        next_validator_index: usize,
    }

    impl<'a> TestBlockIterator<'a> {
        pub fn new(
            block: BlockV2,
            rng: &'a mut TestRng,
            switch_block_indices: Option<Vec<u64>>,
            upgrades_indices: Option<Vec<u64>>,
            validators: Vec<(PublicKey, U512)>,
        ) -> Self {
            let protocol_version = block.protocol_version();
            Self {
                block,
                protocol_version,
                rng,
                switch_block_indices,
                upgrades_indices,
                validators,
                next_validator_index: 0,
            }
        }
    }

    impl<'a> Iterator for TestBlockIterator<'a> {
        type Item = BlockV2;

        fn next(&mut self) -> Option<Self::Item> {
            let (is_successor_of_switch_block, is_upgrade, maybe_validators) = match &self
                .switch_block_indices
            {
                Some(switch_block_heights)
                    if switch_block_heights.contains(&self.block.height()) =>
                {
                    let prev_height = self.block.height().saturating_sub(1);
                    let is_successor_of_switch_block = switch_block_heights.contains(&prev_height);
                    let is_upgrade = is_successor_of_switch_block
                        && self
                            .upgrades_indices
                            .as_ref()
                            .map_or(false, |upgrades_indices| {
                                upgrades_indices.contains(&prev_height)
                            });
                    (
                        is_successor_of_switch_block,
                        is_upgrade,
                        Some(self.validators.clone()),
                    )
                }
                Some(switch_block_heights) => {
                    let prev_height = self.block.height().saturating_sub(1);
                    let is_successor_of_switch_block = switch_block_heights.contains(&prev_height);
                    let is_upgrade = is_successor_of_switch_block
                        && self
                            .upgrades_indices
                            .as_ref()
                            .map_or(false, |upgrades_indices| {
                                upgrades_indices.contains(&prev_height)
                            });
                    (is_successor_of_switch_block, is_upgrade, None)
                }
                None => (false, false, None),
            };

            let maybe_validators = if let Some(validators) = maybe_validators {
                let first_validator = validators.get(self.next_validator_index).unwrap();
                let second_validator = validators.get(self.next_validator_index + 1).unwrap();

                // Put two validators in each switch block.
                let mut validators_for_block = BTreeMap::new();
                validators_for_block.insert(first_validator.0.clone(), first_validator.1);
                validators_for_block.insert(second_validator.0.clone(), second_validator.1);
                self.next_validator_index += 2;

                // If we're out of validators, do round robin on the provided list.
                if self.next_validator_index >= self.validators.len() {
                    self.next_validator_index = 0;
                }
                Some(validators_for_block)
            } else {
                None
            };

            if is_upgrade {
                self.protocol_version = ProtocolVersion::from_parts(
                    self.protocol_version.value().major,
                    self.protocol_version.value().minor + 1,
                    self.protocol_version.value().patch,
                );
            }

            let era_end = maybe_validators.map(|validators| {
                let rnd = EraEndV2::random(self.rng);
                EraEndV2::new(
                    Vec::from(rnd.equivocators()),
                    Vec::from(rnd.inactive_validators()),
                    validators,
                    rnd.rewards().clone(),
                )
            });
            let next_block_era_id = if is_successor_of_switch_block {
                self.block.era_id().successor()
            } else {
                self.block.era_id()
            };
            let count = self.rng.gen_range(0..6);
            let transfer_hashes =
                iter::repeat_with(|| TransactionHash::V1(TransactionV1Hash::random(self.rng)))
                    .take(count)
                    .collect();
            let count = self.rng.gen_range(0..6);
            let staking_hashes =
                iter::repeat_with(|| TransactionHash::V1(TransactionV1Hash::random(self.rng)))
                    .take(count)
                    .collect();
            let count = self.rng.gen_range(0..6);
            let install_upgrade_hashes =
                iter::repeat_with(|| TransactionHash::V1(TransactionV1Hash::random(self.rng)))
                    .take(count)
                    .collect();
            let count = self.rng.gen_range(0..6);
            let standard_hashes =
                iter::repeat_with(|| TransactionHash::V1(TransactionV1Hash::random(self.rng)))
                    .take(count)
                    .collect();

            let next = BlockV2::new(
                *self.block.hash(),
                *self.block.accumulated_seed(),
                *self.block.state_root_hash(),
                self.rng.gen(),
                era_end,
                Timestamp::now(),
                next_block_era_id,
                self.block.height() + 1,
                self.protocol_version,
                PublicKey::random(self.rng),
                transfer_hashes,
                staking_hashes,
                install_upgrade_hashes,
                standard_hashes,
                Default::default(),
            );

            self.block = next.clone();
            Some(next)
        }
    }

    #[test]
    fn should_create_valid_chain() {
        let mut rng = TestRng::new();
        let mut test_block = TestChainSpec::new(&mut rng, None, None, &[]);
        let mut block_batch = test_block.iter().take(100);
        let mut parent_block: BlockV2 = block_batch.next().unwrap();
        for current_block in block_batch {
            assert_eq!(
                current_block.height(),
                parent_block.height() + 1,
                "height should grow monotonically"
            );
            assert_eq!(
                current_block.parent_hash(),
                parent_block.hash(),
                "block's parent should point at previous block"
            );
            parent_block = current_block;
        }
    }

    #[test]
    fn should_create_switch_blocks() {
        let switch_block_indices = vec![0, 10, 76];

        let validators: Vec<_> = iter::repeat_with(crypto::generate_ed25519_keypair)
            .take(2)
            .map(|(secret_key, public_key)| ValidatorSpec {
                secret_key,
                public_key,
                weight: None,
            })
            .collect();

        let mut rng = TestRng::new();
        let mut test_block = TestChainSpec::new(
            &mut rng,
            Some(switch_block_indices.clone()),
            None,
            &validators,
        );
        let block_batch: Vec<_> = test_block.iter().take(100).collect();

        let base_height = block_batch.first().expect("should have block").height();

        for block in block_batch {
            if switch_block_indices
                .iter()
                .map(|index| index + base_height)
                .any(|index| index == block.height())
            {
                assert!(block.is_switch_block())
            } else {
                assert!(!block.is_switch_block())
            }
        }
    }
}
