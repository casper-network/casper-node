use thiserror::Error;
use tracing::{error, warn};

use casper_types::{crypto, EraId};

use crate::{
    components::network::blocklist::BlocklistJustification,
    effect::{
        announcements::{FatalAnnouncement, PeerBehaviorAnnouncement},
        EffectBuilder, EffectExt, Effects,
    },
    fatal,
    types::{BlockHash, BlockValidationError, MetaBlockMergeError, NodeId},
};

use super::event::Event;

#[derive(Error, Debug)]
pub(crate) enum InvalidGossipError {
    #[error("received cryptographically invalid block for: {block_hash} from: {peer} with error: {validation_error}")]
    Block {
        block_hash: BlockHash,
        peer: NodeId,
        validation_error: BlockValidationError,
    },
    #[error("received cryptographically invalid finality_signature for: {block_hash} from: {peer} with error: {validation_error}")]
    FinalitySignature {
        block_hash: BlockHash,
        peer: NodeId,
        validation_error: crypto::Error,
    },
}

impl InvalidGossipError {
    pub(super) fn peer(&self) -> NodeId {
        match self {
            InvalidGossipError::FinalitySignature { peer, .. }
            | InvalidGossipError::Block { peer, .. } => *peer,
        }
    }
}

#[derive(Error, Debug)]
pub(crate) enum Bogusness {
    #[error("peer is not a validator in current era")]
    NotAValidator,
    #[error("peer provided finality signatures from incorrect era")]
    SignatureEraIdMismatch,
}

#[derive(Error, Debug)]
pub(crate) enum Error {
    #[error(transparent)]
    InvalidGossip(Box<InvalidGossipError>),
    #[error("invalid configuration")]
    InvalidConfiguration,
    #[error("mismatched eras detected")]
    EraMismatch {
        block_hash: BlockHash,
        expected: EraId,
        actual: EraId,
        peer: NodeId,
    },
    #[error("mismatched block hash: expected={expected}, actual={actual}")]
    BlockHashMismatch {
        expected: BlockHash,
        actual: BlockHash,
    },
    #[error("should not be possible to have sufficient finality without block: {block_hash}")]
    SufficientFinalityWithoutBlock { block_hash: BlockHash },
    #[error("bogus validator detected")]
    BogusValidator(Bogusness),
    #[error(transparent)]
    MetaBlockMerge(#[from] MetaBlockMergeError),
    #[error("tried to insert a signature past the bounds")]
    TooManySignatures { peer: NodeId, limit: u32 },
}

impl Error {
    pub fn effects<REv>(self, effect_builder: EffectBuilder<REv>) -> Effects<Event>
    where
        REv: From<PeerBehaviorAnnouncement> + From<FatalAnnouncement> + Send,
    {
        let error = self;
        match error {
            Error::InvalidGossip(ref gossip_error) => {
                warn!(%gossip_error, "received invalid block");
                effect_builder
                    .announce_block_peer_with_justification(
                        gossip_error.peer(),
                        BlocklistJustification::SentBadBlock { error },
                    )
                    .ignore()
            }
            Error::EraMismatch {
                peer,
                block_hash,
                expected,
                actual,
            } => {
                warn!(
                    "era mismatch from {} for {}; expected: {} and actual: {}",
                    peer, block_hash, expected, actual
                );
                effect_builder
                    .announce_block_peer_with_justification(
                        peer,
                        BlocklistJustification::SentBadBlock { error },
                    )
                    .ignore()
            }
            ref error @ Error::BlockHashMismatch { .. } => {
                error!(%error, "finality signature has mismatched block_hash; this is a bug");
                Effects::new()
            }
            ref error @ Error::SufficientFinalityWithoutBlock { .. } => {
                error!(%error, "should not have sufficient finality without block");
                Effects::new()
            }
            Error::InvalidConfiguration => fatal!(
                effect_builder,
                "node has an invalid configuration, shutting down"
            )
            .ignore(),
            Error::BogusValidator(_) => {
                error!(%error, "unexpected detection of bogus validator, this is a bug");
                Effects::new()
            }
            Error::MetaBlockMerge(error) => {
                error!(%error, "failed to merge meta blocks, this is a bug");
                Effects::new()
            }
            Error::TooManySignatures { peer, limit } => effect_builder
                .announce_block_peer_with_justification(
                    peer,
                    BlocklistJustification::SentTooManyFinalitySignatures { max_allowed: limit },
                )
                .ignore(),
        }
    }
}
