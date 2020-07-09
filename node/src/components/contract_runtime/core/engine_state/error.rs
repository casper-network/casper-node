use thiserror::Error;

use crate::components::contract_runtime::shared::newtypes::Blake2bHash;
use crate::components::contract_runtime::shared::wasm_prep;
use types::ProtocolVersion;
use types::{bytesrepr, system_contract_errors::mint};

use crate::components::contract_runtime::core::execution;
use crate::components::contract_runtime::storage;

#[derive(Error, Debug)]
pub enum Error {
    #[error("Invalid hash length: expected {expected}, actual {actual}")]
    InvalidHashLength { expected: usize, actual: usize },
    #[error("Invalid account hash length: expected {expected}, actual {actual}")]
    InvalidAccountHashLength { expected: usize, actual: usize },
    #[error("Invalid protocol version: {0}")]
    InvalidProtocolVersion(ProtocolVersion),
    #[error("Invalid upgrade config")]
    InvalidUpgradeConfig,
    #[error("Wasm preprocessing error: {0}")]
    WasmPreprocessing(wasm_prep::PreprocessingError),
    #[error("Wasm serialization error: {0:?}")]
    WasmSerialization(parity_wasm::SerializationError),
    #[error(transparent)]
    Exec(execution::Error),
    #[error("Storage error: {}", _0)]
    Storage(storage::error::Error),
    #[error("Authorization failure: not authorized.")]
    Authorization,
    #[error("Insufficient payment")]
    InsufficientPayment,
    #[error("Deploy error")]
    Deploy,
    #[error("Payment finalization error")]
    Finalization,
    #[error("Missing system contract association: {}", _0)]
    MissingSystemContract(String),
    #[error("Serialization error: {}", _0)]
    Serialization(bytesrepr::Error),
    #[error("Mint error: {}", _0)]
    Mint(mint::Error),
    #[error("Unsupported key type: {}", _0)]
    InvalidKeyVariant(String),
    #[error("Invalid upgrade result value")]
    InvalidUpgradeResult,
    #[error("Unsupported deploy item variant: {}", _0)]
    InvalidDeployItemVariant(String),
}

impl From<wasm_prep::PreprocessingError> for Error {
    fn from(error: wasm_prep::PreprocessingError) -> Self {
        Error::WasmPreprocessing(error)
    }
}

impl From<parity_wasm::SerializationError> for Error {
    fn from(error: parity_wasm::SerializationError) -> Self {
        Error::WasmSerialization(error)
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

impl From<storage::error::Error> for Error {
    fn from(error: storage::error::Error) -> Self {
        Error::Storage(error)
    }
}

impl From<bytesrepr::Error> for Error {
    fn from(error: bytesrepr::Error) -> Self {
        Error::Serialization(error)
    }
}

impl From<mint::Error> for Error {
    fn from(error: mint::Error) -> Self {
        Error::Mint(error)
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RootNotFound(Blake2bHash);

impl RootNotFound {
    pub fn new(hash: Blake2bHash) -> Self {
        RootNotFound(hash)
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.0.to_vec()
    }
}
