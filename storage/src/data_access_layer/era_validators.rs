//! Support for querying era validators.

use crate::tracking_copy::TrackingCopyError;
use casper_types::{system::auction::EraValidators, Digest, ProtocolVersion};
use std::fmt::{Display, Formatter};

/// Request for era validators.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EraValidatorsRequest {
    state_hash: Digest,
    protocol_version: ProtocolVersion,
}

impl EraValidatorsRequest {
    /// Constructs a new EraValidatorsRequest.
    pub fn new(state_hash: Digest, protocol_version: ProtocolVersion) -> Self {
        EraValidatorsRequest {
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

/// Result enum that represents all possible outcomes of a balance request.
#[derive(Debug)]
pub enum EraValidatorsResult {
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
        /// Era Validators.
        era_validators: EraValidators,
    },
}

impl EraValidatorsResult {
    /// Returns true if success.
    pub fn is_success(&self) -> bool {
        matches!(self, EraValidatorsResult::Success { .. })
    }

    /// Takes era validators.
    pub fn take_era_validators(self) -> Option<EraValidators> {
        match self {
            EraValidatorsResult::AuctionNotFound
            | EraValidatorsResult::RootNotFound
            | EraValidatorsResult::ValueNotFound(_)
            | EraValidatorsResult::Failure(_) => None,
            EraValidatorsResult::Success { era_validators } => Some(era_validators),
        }
    }
}

impl Display for EraValidatorsResult {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            EraValidatorsResult::AuctionNotFound => write!(f, "system auction not found"),
            EraValidatorsResult::RootNotFound => write!(f, "state root not found"),
            EraValidatorsResult::ValueNotFound(msg) => write!(f, "value not found: {}", msg),
            EraValidatorsResult::Failure(tce) => write!(f, "{}", tce),
            EraValidatorsResult::Success { .. } => {
                write!(f, "success")
            }
        }
    }
}
