use thiserror::Error;

use crate::components::contract_runtime::{
    core::execution,
    shared::{newtypes::Blake2bHash, wasm_prep},
    storage,
};

use types::{bytesrepr, system_contract_errors::mint, ProtocolVersion};

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
    WasmPreprocessing(#[from] wasm_prep::PreprocessingError),
    #[error("Wasm serialization error: {0:?}")]
    WasmSerialization(#[from] parity_wasm::SerializationError),
    #[error(transparent)]
    Exec(execution::Error),
    #[error("Storage error: {0}")]
    Storage(#[from] storage::error::Error),
    #[error("Authorization failure: not authorized.")]
    Authorization,
    #[error("Insufficient payment")]
    InsufficientPayment,
    #[error("Deploy error")]
    Deploy,
    #[error("Payment finalization error")]
    Finalization,
    #[error("Missing system contract association: {0}")]
    MissingSystemContract(String),
    #[error("Bytesrepr error: {0}")]
    Bytesrepr(String),
    #[error("bincode serialization: {0}")]
    BincodeSerialization(#[source] bincode::ErrorKind),
    #[error("bincode deserialization: {0}")]
    BincodeDeserialization(#[source] bincode::ErrorKind),
    #[error("Mint error: {0}")]
    Mint(String),
    #[error("Unsupported key type: {0}")]
    InvalidKeyVariant(String),
    #[error("Invalid upgrade result value")]
    InvalidUpgradeResult,
    #[error("Unsupported deploy item variant: {0}")]
    InvalidDeployItemVariant(String),
}

impl Error {
    pub(crate) fn from_serialization(error: bincode::ErrorKind) -> Self {
        Error::BincodeSerialization(error)
    }

    // TODO - remove lint relaxation, or remove method if not required
    #[allow(dead_code)]
    pub(crate) fn from_deserialization(error: bincode::ErrorKind) -> Self {
        Error::BincodeDeserialization(error)
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

#[derive(Debug, PartialEq, Eq, Clone)]
pub struct RootNotFound(Blake2bHash);

impl RootNotFound {
    pub fn new(hash: Blake2bHash) -> Self {
        RootNotFound(hash)
    }

    pub fn to_vec(&self) -> Vec<u8> {
        self.0.as_ref().to_vec()
    }
}
