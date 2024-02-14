use crate::tracking_copy::TrackingCopyError;
use casper_types::Digest;

pub const EXECUTION_RESULTS_CHECKSUM_NAME: &str = "execution_results_checksum";

/// Represents a request to obtain current execution results checksum.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionResultsChecksumRequest {
    state_hash: Digest,
}

impl ExecutionResultsChecksumRequest {
    /// Creates new request.
    pub fn new(state_hash: Digest) -> Self {
        ExecutionResultsChecksumRequest { state_hash }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }
}

/// Represents a result of a `execution_results_checksum` request.
#[derive(Debug)]
pub enum ExecutionResultsChecksumResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Returned if system registry is not found.
    RegistryNotFound,
    /// Returned if checksum is not found.
    ChecksumNotFound,
    /// Contains current checksum returned from the global state.
    Success {
        /// Current checksum.
        checksum: Digest,
    },
    /// Error occurred.
    Failure(TrackingCopyError),
}

impl ExecutionResultsChecksumResult {
    /// Returns a Result matching the original api for this functionality.
    pub fn as_legacy(&self) -> Result<Option<Digest>, TrackingCopyError> {
        match self {
            ExecutionResultsChecksumResult::RootNotFound
            | ExecutionResultsChecksumResult::RegistryNotFound
            | ExecutionResultsChecksumResult::ChecksumNotFound => Ok(None),
            ExecutionResultsChecksumResult::Success { checksum } => Ok(Some(*checksum)),
            ExecutionResultsChecksumResult::Failure(err) => Err(err.clone()),
        }
    }
}
