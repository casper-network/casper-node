//! Blocklisting support.
//!
//! Blocked peers are prevented from interacting with the node through a variety of means.

use std::fmt::{self, Display, Formatter};

use casper_hashing::Digest;
use casper_types::EraId;
use datasize::DataSize;
use serde::Serialize;

use crate::{
    components::block_accumulator,
    types::{DeployId, Tag},
};

/// Reasons why a peer was blocked.
#[derive(DataSize, Debug, Serialize)]
pub(crate) enum BlocklistJustification {
    /// Peer sent incorrect item.
    SentBadItem { tag: Tag },
    /// A finality signature that was sent is invalid.
    SentBadFinalitySignature {
        /// Error reported by block accumulator.
        #[serde(skip_serializing)]
        #[data_size(skip)]
        error: block_accumulator::Error,
    },
    /// A block that was sent is invalid.
    SentBadBlock {
        /// Error reported by block accumulator.
        #[serde(skip_serializing)]
        #[data_size(skip)]
        error: block_accumulator::Error,
    },
    /// A serialized deploy was received that failed to deserialize.
    SentBadDeploy {
        /// The deserialization error.
        #[serde(skip_serializing)]
        #[data_size(skip)]
        error: bincode::Error,
    },
    /// An invalid consensus value was received.
    SentInvalidConsensusValue {
        /// The era for which the invalid value was destined.
        era: EraId,
    },
    /// An invalid consensus message was received.
    SentInvalidConsensusMessage {
        /// Actual validation error.
        #[serde(skip_serializing)]
        #[data_size(skip)]
        error: anyhow::Error,
    },
    /// Peer misbehaved during consensus and is blocked for it.
    BadConsensusBehavior,
    /// Peer is on the wrong network.
    WrongNetwork {
        /// The network name reported by the peer.
        peer_network_name: String,
    },
    /// Peer presented the wrong chainspec hash.
    WrongChainspecHash {
        /// The chainspec hash reported by the peer.
        peer_chainspec_hash: Digest,
    },
    /// Peer did not present a chainspec hash.
    MissingChainspecHash,
    /// Peer is considered dishonest.
    DishonestPeer,
    /// Peer has a deploy but is not providing it.
    PeerDidNotProvideADeploy { deploy_id: DeployId },
}

impl Display for BlocklistJustification {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlocklistJustification::SentBadItem { tag } => {
                write!(f, "sent a {:?} item we couldn't parse", tag)
            }
            BlocklistJustification::SentBadFinalitySignature { error } => write!(
                f,
                "sent a finality signature that is invalid or unexpected ({})",
                error
            ),
            BlocklistJustification::SentBadDeploy { error } => {
                write!(f, "sent a deploy that could not be deserialized: {}", error)
            }
            BlocklistJustification::SentInvalidConsensusValue { era } => {
                write!(f, "sent an invalid consensus value in {}", era)
            }
            BlocklistJustification::SentInvalidConsensusMessage { error } => {
                write!(f, "sent an invalid consensus message: {}", error)
            }
            BlocklistJustification::BadConsensusBehavior => {
                f.write_str("sent invalid data in consensus")
            }
            BlocklistJustification::WrongNetwork { peer_network_name } => write!(
                f,
                "reported to be on the wrong network ({:?})",
                peer_network_name
            ),
            BlocklistJustification::WrongChainspecHash {
                peer_chainspec_hash,
            } => write!(
                f,
                "reported a mismatched chainspec hash ({})",
                peer_chainspec_hash
            ),
            BlocklistJustification::MissingChainspecHash => {
                f.write_str("sent handshake without chainspec hash")
            }
            BlocklistJustification::SentBadBlock { error } => {
                write!(f, "sent a block that is invalid or unexpected ({})", error)
            }
            BlocklistJustification::DishonestPeer => {
                f.write_str("sent handshake without chainspec hash")
            }
            BlocklistJustification::PeerDidNotProvideADeploy { deploy_id } => {
                write!(
                    f,
                    "peer has a deploy but is not providing it ({})",
                    deploy_id
                )
            }
        }
    }
}
