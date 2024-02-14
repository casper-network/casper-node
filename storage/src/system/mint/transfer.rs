use crate::tracking_copy::TrackingCopyError;
use casper_types::{Digest, URef, U512};

/// Request for motes transfer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransferRequest {
    state_hash: Digest,
    source: URef,
    target: URef,
    amount: U512,
}

impl TransferRequest {
    /// Creates new request object.
    pub fn new(state_hash: Digest, source: URef, target: URef, amount: U512) -> Self {
        TransferRequest {
            state_hash,
            source,
            target,
            amount,
        }
    }

    /// Returns state root hash.
    pub fn state_hash(&self) -> Digest {
        self.state_hash
    }

    /// Returns source uref.
    pub fn source(&self) -> URef {
        self.source
    }

    /// Returns target uref.
    pub fn target(&self) -> URef {
        self.target
    }

    /// Returns amount.
    pub fn amount(&self) -> U512 {
        self.amount
    }
}

#[derive(Debug)]
pub enum TransferResult {
    /// Invalid state root hash.
    RootNotFound,
    /// Transfer succeeded
    Success,
    /// Transfer failed
    Failure,
    /// Storage Error
    StorageError(crate::global_state::error::Error),
    /// Tracking Copy Error
    TrackingCopyError(TrackingCopyError),
}
