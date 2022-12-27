//! Blocklisting support.
//!
//! Blocked peers are prevented from interacting with the node through a variety of means.

use std::fmt::{self, Display, Formatter};

use casper_hashing::Digest;
use casper_types::EraId;
use datasize::DataSize;
use serde::Serialize;

use crate::{components::block_accumulator, types::Tag};

/// Reasons why a peer was blocked.
#[derive(DataSize, Debug, Serialize)]
pub(crate) enum BlocklistJustification {
    /// Peer sent incorrect item.
    SentBadItem { tag: Tag },
    /// Peer sent an item which failed validation.
    SentInvalidItem { tag: Tag, error_msg: String },
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
    /// An invalid consensus value was received.
    SentInvalidConsensusValue {
        /// The era for which the invalid value was destined.
        era: EraId,
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
}

impl Display for BlocklistJustification {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            BlocklistJustification::SentBadItem { tag } => {
                write!(f, "sent a {} we couldn't parse", tag)
            }
            BlocklistJustification::SentInvalidItem { tag, error_msg } => {
                write!(f, "sent a {} which failed validation ({})", tag, error_msg)
            }
            BlocklistJustification::SentBadFinalitySignature { error } => write!(
                f,
                "sent a finality signature that is invalid or unexpected ({})",
                error
            ),
            BlocklistJustification::SentInvalidConsensusValue { era } => {
                write!(f, "sent an invalid consensus value in {}", era)
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
            BlocklistJustification::DishonestPeer => f.write_str("dishonest peer"),
        }
    }
}
