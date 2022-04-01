//! Definition of all the possible outcomes of the operation on an `EngineState` instance.
use datasize::DataSize;
use thiserror::Error;

use casper_hashing::Digest;
use casper_types::{bytesrepr, system::mint, ApiError, ProtocolVersion};

use crate::{
    core::{
        engine_state::{genesis::GenesisError, upgrade::ProtocolUpgradeError},
        execution,
        runtime::stack,
    },
    shared::wasm_prep,
    storage,
    storage::global_state::CommitError,
};

/// Engine state errors.
#[derive(Clone, Error, Debug)]
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
    WasmPreprocessing(#[from] wasm_prep::PreprocessingError),
    /// WASM serialization error.
    #[error("Wasm serialization error: {0:?}")]
    WasmSerialization(#[from] parity_wasm::SerializationError),
    /// Contract execution error.
    #[error(transparent)]
    Exec(execution::Error),
    /// Storage error.
    #[error("Storage error: {0}")]
    Storage(#[from] storage::error::Error),
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
    ProtocolUpgrade(ProtocolUpgradeError),
    /// Invalid deploy item variant.
    #[error("Unsupported deploy item variant: {0}")]
    InvalidDeployItemVariant(String),
    /// Commit error.
    #[error(transparent)]
    CommitError(#[from] CommitError),
    /// Missing system contract registry.
    #[error("Missing SystemContractRegistry")]
    MissingSystemContractRegistry,
    /// Missing system contract hash.
    #[error("Missing system contract hash: {0}")]
    MissingSystemContractHash(String),
    /// An attempt to push to the runtime stack while already at the maximum height.
    #[error("Runtime stack overflow")]
    RuntimeStackOverflow,
    /// Failed to get the set of Key::Withdraw from global state.
    #[error("Failed to get withdraw keys")]
    FailedToGetWithdrawKeys,
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

impl From<lmdb::Error> for Error {
    fn from(error: lmdb::Error) -> Self {
        Error::Storage(storage::error::Error::Lmdb(error))
    }
}

impl From<mint::Error> for Error {
    fn from(error: mint::Error) -> Self {
        Error::Mint(format!("{}", error))
    }
}

impl From<GenesisError> for Error {
    fn from(genesis_error: GenesisError) -> Self {
        Self::Genesis(Box::new(genesis_error))
    }
}

impl From<stack::RuntimeStackOverflow> for Error {
    fn from(_: stack::RuntimeStackOverflow) -> Self {
        Self::RuntimeStackOverflow
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
