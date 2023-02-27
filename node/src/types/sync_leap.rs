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

        let mut verified: Vec<BlockHash> = vec![self.trusted_block_header.block_hash()];

        while let Some(hash) = verified.pop() {
            if let Some(header) = headers.remove(&hash) {
                verified.push(*header.parent_hash());
                if let Some(mut validator_weights) = header.next_era_validator_weights() {
                    // TODO[RC]: Don't forget to uncomment this!

                    // If this is a switch block right before the upgrade to the current protocol
                    // version, and if this upgrade changes the validator set, use the validator
                    // weights from the chainspec.

                    // if header.next_block_era_id() == validation_metadata.activation_point.era_id() {
                    //     if let Some(updated_weights) = validation_metadata
                    //         .global_state_update
                    //         .as_ref()
                    //         .and_then(|update| update.validators.as_ref())
                    //     {
                    //         validator_weights = updated_weights
                    //     }
                    // }

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

    use std::iter;

    use casper_types::{crypto, testing::TestRng, PublicKey, SecretKey};
    use itertools::Itertools;
    use num_rational::Ratio;

    use crate::{
        components::fetcher::FetchItem,
        tests::TestChainSpec,
        types::{
            sync_leap::SyncLeapValidationError, ActivationPoint, Block, BlockHeader,
            BlockHeaderWithMetadata, BlockSignatures, FinalitySignature,
            SyncLeapValidationMetaData,
        },
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

    fn make_signed_block_header(
        height: usize,
        test_chain: &[Block],
        validators: &[(SecretKey, PublicKey)],
    ) -> BlockHeaderWithMetadata {
        let header = test_chain.get(height).unwrap().header().clone();
        let hash = header.block_hash();
        let era_id = header.era_id();
        let mut block_signatures = BlockSignatures::new(hash, era_id);
        validators.iter().for_each(|(secret_key, public_key)| {
            let finality_signature =
                FinalitySignature::create(hash, era_id, &secret_key, public_key.clone());
            block_signatures.insert_proof(public_key.clone(), finality_signature.signature);
        });

        BlockHeaderWithMetadata {
            block_header: header,
            block_signatures,
        }
    }

    fn make_test_sync_leap(
        rng: &mut TestRng,
        switch_blocks: &[u64],
        query: usize,
        trusted_ancestor_headers: &[usize],
        signed_block_headers: &[usize],
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
            .map(|height| make_signed_block_header(*height, &test_chain, &validators))
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
        let sync_leap = make_test_sync_leap(
            &mut rng,
            &switch_blocks,
            query,
            &trusted_ancestor_headers,
            &signed_block_headers,
        );

        let result = sync_leap.validate(&validation_metadata);
        assert!(result.is_ok());

        // Querying for a switch block.
        let query = 6;
        let trusted_ancestor_headers = [5, 4, 3];
        let signed_block_headers = [9, 11];
        let sync_leap = make_test_sync_leap(
            &mut rng,
            &switch_blocks,
            query,
            &trusted_ancestor_headers,
            &signed_block_headers,
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
}
