//! Definition of all the possible outcomes of the operation on an `EngineState` instance.
use datasize::DataSize;
use thiserror::Error;

use casper_storage::{
    global_state::{self, state::CommitError},
    system::{
        genesis::GenesisError, protocol_upgrade::ProtocolUpgradeError, transfer::TransferError,
    },
    tracking_copy::TrackingCopyError,
};
use casper_types::{
    account::AccountHash, binary_port, bytesrepr, system::mint, ApiError, Digest, Key, KeyTag,
    PackageHash, ProtocolVersion,
};

use crate::{
    execution,
    runtime::{stack, PreprocessingError},
};

/// Engine state errors.
#[derive(Clone, Error, Debug)]
#[non_exhaustive]
pub enum Error {
    /// Specified state root hash is not found.
    #[error("Root not found: {0}")]
    RootNotFound(Digest),
    /// Protocol version used in the deploy is invalid.
    #[error("Invalid protocol version: {0}")]
    InvalidProtocolVersion(ProtocolVersion),
    /// Genesis error.
    #[error("{0:?}")]
    Genesis(Box<GenesisError>),
    /// WASM preprocessing error.
    #[error("Wasm preprocessing error: {0}")]
    WasmPreprocessing(#[from] PreprocessingError),
    /// WASM serialization error.
    #[error("Wasm serialization error: {0:?}")]
    WasmSerialization(#[from] casper_wasm::SerializationError),
    /// Contract execution error.
    #[error(transparent)]
    Exec(execution::Error),
    /// Storage error.
    #[error("Storage error: {0}")]
    Storage(#[from] global_state::error::Error),
    /// Authorization error.
    #[error("Authorization failure: not authorized.")]
    Authorization,
    /// Payment code provided insufficient funds for execution.
    #[error("Insufficient payment")]
    InsufficientPayment,
    /// Motes to gas conversion resulted in an overflow.
    #[error("Gas conversion overflow")]
    GasConversionOverflow,
    /// General deploy error.
    #[error("Deploy error")]
    Deploy,
    /// Executing a payment finalization code resulted in an error.
    #[error("Payment finalization error")]
    Finalization,
    /// Serialization/deserialization error.
    #[error("Bytesrepr error: {0}")]
    Bytesrepr(String),
    /// Mint error.
    #[error("Mint error: {0}")]
    Mint(String),
    /// Invalid key variant.
    #[error("Unsupported key type")]
    InvalidKeyVariant,
    /// Protocol upgrade error.
    #[error("Protocol upgrade error: {0}")]
    ProtocolUpgrade(#[from] ProtocolUpgradeError),
    /// Invalid deploy item variant.
    #[error("Unsupported deploy item variant: {0}")]
    InvalidDeployItemVariant(String),
    /// Commit error.
    #[error(transparent)]
    CommitError(#[from] CommitError),
    /// Missing system contract registry.
    #[error("Missing system contract registry")]
    MissingSystemContractRegistry,
    /// Missing system contract hash.
    #[error("Missing system contract hash: {0}")]
    MissingSystemContractHash(String),
    /// Missing checksum registry.
    #[error("Missing checksum registry")]
    MissingChecksumRegistry,
    /// An attempt to push to the runtime stack while already at the maximum height.
    #[error("Runtime stack overflow")]
    RuntimeStackOverflow,
    /// Failed to get the set of keys matching the specified tag.
    #[error("Failed to get keys of kind: {0:?}")]
    FailedToGetKeys(KeyTag),
    /// Failed to get the purses stored under Key::Withdraw
    #[error("Failed to get stored values under withdraws")]
    FailedToGetStoredWithdraws,
    /// Failed to convert the StoredValue into WithdrawPurse.
    #[error("Failed to convert the stored value to a withdraw purse")]
    FailedToGetWithdrawPurses,
    /// Failed to retrieve the unbonding delay from the auction state.
    #[error("Failed to retrieve the unbonding delay from the auction state")]
    FailedToRetrieveUnbondingDelay,
    /// Failed to retrieve the current EraId from the auction state.
    #[error("Failed to retrieve the era_id from the auction state")]
    FailedToRetrieveEraId,
    /// Failed to put a trie node into global state because some of its children were missing.
    #[error("Failed to put a trie into global state because some of its children were missing")]
    MissingTrieNodeChildren(Vec<Digest>),
    /// Failed to retrieve contract record by a given account hash.
    #[error("Failed to retrieve contract by account hash {0}")]
    MissingContractByAccountHash(AccountHash),
    /// Failed to retrieve the entity's package
    #[error("Failed to retrieve the entity package as {0}")]
    MissingEntityPackage(PackageHash),
    /// Failed to retrieve accumulation purse from handle payment system contract.
    #[error("Failed to retrieve accumulation purse from the handle payment contract")]
    FailedToRetrieveAccumulationPurse,
    /// Failed to prune listed keys.
    #[error("Pruning attempt failed.")]
    FailedToPrune(Vec<Key>),
    /// Storage error.
    #[error("Tracking copy error: {0}")]
    TrackingCopy(TrackingCopyError),
    /// Native transfer error.
    #[error("Transfer error: {0}")]
    Transfer(TransferError),
}

impl Error {
    /// Creates an [`enum@Error`] instance of an [`Error::Exec`] variant with an API
    /// error-compatible object.
    ///
    /// This method should be used only by native code that has to mimic logic of a WASM executed
    /// code.
    pub fn reverter(api_error: impl Into<ApiError>) -> Error {
        Error::Exec(execution::Error::Revert(api_error.into()))
    }
}

impl From<TransferError> for Error {
    fn from(err: TransferError) -> Self {
        Error::Transfer(err)
    }
}

impl From<execution::Error> for Error {
    fn from(error: execution::Error) -> Self {
        match error {
            execution::Error::WasmPreprocessing(preprocessing_error) => {
                Error::WasmPreprocessing(preprocessing_error)
            }
            _ => Error::Exec(error),
        }
    }
}

impl From<bytesrepr::Error> for Error {
    fn from(error: bytesrepr::Error) -> Self {
        Error::Bytesrepr(format!("{}", error))
    }
}

impl From<mint::Error> for Error {
    fn from(error: mint::Error) -> Self {
        Error::Mint(format!("{}", error))
    }
}

impl From<Box<GenesisError>> for Error {
    fn from(genesis_error: Box<GenesisError>) -> Self {
        Self::Genesis(genesis_error)
    }
}

impl From<stack::RuntimeStackOverflow> for Error {
    fn from(_: stack::RuntimeStackOverflow) -> Self {
        Self::RuntimeStackOverflow
    }
}

impl From<TrackingCopyError> for Error {
    fn from(e: TrackingCopyError) -> Self {
        Error::TrackingCopy(e)
    }
}

impl From<Error> for binary_port::ErrorCode {
    fn from(err: Error) -> Self {
        match err {
            Error::RootNotFound(_) => binary_port::ErrorCode::RootNotFound,
            Error::InvalidDeployItemVariant(_) => binary_port::ErrorCode::InvalidDeployItemVariant,
            Error::WasmPreprocessing(_) => binary_port::ErrorCode::WasmPreprocessing,
            Error::InvalidProtocolVersion(_) => binary_port::ErrorCode::UnsupportedProtocolVersion,
            Error::Deploy => binary_port::ErrorCode::InvalidTransaction,
            _ => binary_port::ErrorCode::InternalError,
        }
    }
}

impl DataSize for Error {
    const IS_DYNAMIC: bool = true;

    const STATIC_HEAP_SIZE: usize = 0;

    // TODO
    #[inline]
    fn estimate_heap_size(&self) -> usize {
        12 // TODO: replace with some actual estimation depending on the variant
    }
}
