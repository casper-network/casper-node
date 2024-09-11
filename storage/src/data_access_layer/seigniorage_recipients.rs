//! Support for querying seigniorage recipients.

use crate::tracking_copy::TrackingCopyError;
use casper_types::{system::auction::SeigniorageRecipientsSnapshot, Digest, ProtocolVersion};
use std::fmt::{Display, Formatter};

/// Request for seigniorage recipients.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeigniorageRecipientsRequest {
    state_hash: Digest,
    protocol_version: ProtocolVersion,
}

impl SeigniorageRecipientsRequest {
    /// Constructs a new SeigniorageRecipientsRequest.
    pub fn new(state_hash: Digest, protocol_version: ProtocolVersion) -> Self {
        SeigniorageRecipientsRequest {
            state_hash,
            protocol_version,
        }
    }

    /// Get the state hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Get the protocol version.
    pub fn protocol_version(&self) -> ProtocolVersion {
        self.protocol_version
    }
}

/// Result enum that represents all possible outcomes of a seignorage recipients request.
#[derive(Debug)]
pub enum SeigniorageRecipientsResult {
    /// Returned if auction is not found. This is a catastrophic outcome.
    AuctionNotFound,
    /// Returned if a passed state root hash is not found. This is recoverable.
    RootNotFound,
    /// Value not found. This is not erroneous if the record does not exist.
    ValueNotFound(String),
    /// There is no systemic issue, but the query itself errored.
    Failure(TrackingCopyError),
    /// The query succeeded.
    Success {
        /// Seigniorage recipients.
        seigniorage_recipients: SeigniorageRecipientsSnapshot,
    },
}

impl SeigniorageRecipientsResult {
    /// Returns true if success.
    pub fn is_success(&self) -> bool {
        matches!(self, SeigniorageRecipientsResult::Success { .. })
    }

    /// Takes seigniorage recipients.
    pub fn into_option(self) -> Option<SeigniorageRecipientsSnapshot> {
        match self {
            SeigniorageRecipientsResult::AuctionNotFound
            | SeigniorageRecipientsResult::RootNotFound
            | SeigniorageRecipientsResult::ValueNotFound(_)
            | SeigniorageRecipientsResult::Failure(_) => None,
            SeigniorageRecipientsResult::Success {
                seigniorage_recipients,
            } => Some(seigniorage_recipients),
        }
    }
}

impl Display for SeigniorageRecipientsResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            SeigniorageRecipientsResult::AuctionNotFound => write!(f, "system auction not found"),
            SeigniorageRecipientsResult::RootNotFound => write!(f, "state root not found"),
            SeigniorageRecipientsResult::ValueNotFound(msg) => {
                write!(f, "value not found: {}", msg)
            }
            SeigniorageRecipientsResult::Failure(tce) => write!(f, "{}", tce),
            SeigniorageRecipientsResult::Success { .. } => {
                write!(f, "success")
            }
        }
    }
}
