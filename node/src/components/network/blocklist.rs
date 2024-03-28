//! Blocklisting support.
//!
//! Blocked peers are prevented from interacting with the node through a variety of means.

use std::fmt::{self, Display, Formatter};

use casper_types::EraId;
use datasize::DataSize;
use serde::Serialize;

use crate::{
    components::{block_accumulator, fetcher::Tag},
    consensus::ValidationError,
    utils::display_error,
};

/// Reasons why a peer was blocked.
#[derive(Clone, DataSize, Debug, Serialize)]
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
        //// Cause of value invalidity.
        cause: ValidationError,
    },
    /// Peer misbehaved during consensus and is blocked for it.
    BadConsensusBehavior,
    /// Peer is considered dishonest.
    DishonestPeer,
    /// Peer sent too many finality signatures.
    SentTooManyFinalitySignatures { max_allowed: u32 },
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
            BlocklistJustification::SentInvalidConsensusValue { era, cause } => {
                write!(
                    f,
                    "sent an invalid consensus value in {}: {}",
                    era,
                    display_error(cause)
                )
            }
            BlocklistJustification::BadConsensusBehavior => {
                f.write_str("sent invalid data in consensus")
            }
            BlocklistJustification::SentBadBlock { error } => {
                write!(f, "sent a block that is invalid or unexpected ({})", error)
            }
            BlocklistJustification::DishonestPeer => f.write_str("dishonest peer"),
            BlocklistJustification::SentTooManyFinalitySignatures { max_allowed } => write!(
                f,
                "sent too many finality signatures: maximum {max_allowed} signatures are allowed"
            ),
        }
    }
}
