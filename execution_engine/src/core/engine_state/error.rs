use datasize::DataSize;
use thiserror::Error;

use casper_types::{bytesrepr, system::mint, ProtocolVersion};

use crate::{
    core::{
        engine_state::{genesis::GenesisError, upgrade::ProtocolUpgradeError},
        execution,
    },
    shared::{newtypes::Blake2bHash, wasm_prep},
    storage,
};

#[derive(Clone, Error, Debug)]
pub enum Error {
    #[error("Invalid hash length: expected {expected}, actual {actual}")]
    InvalidHashLength { expected: usize, actual: usize },
    #[error("Invalid account hash length: expected {expected}, actual {actual}")]
    InvalidAccountHashLength { expected: usize, actual: usize },
    #[error("Invalid protocol version: {0}")]
    InvalidProtocolVersion(ProtocolVersion),
    #[error("Genesis error.")]
    Genesis(Box<GenesisError>),
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
    #[error("Gas conversion overflow")]
    GasConversionOverflow,
    #[error("Deploy error")]
    Deploy,
    #[error("Payment finalization error")]
    Finalization,
    #[error("Missing system contract association: {0}")]
    MissingSystemContract(String),
    #[error("Bytesrepr error: {0}")]
    Bytesrepr(String),
    #[error("Mint error: {0}")]
    Mint(String),
    #[error("Unsupported key type")]
    InvalidKeyVariant,
    #[error("Protocol upgrade error: {0}")]
    ProtocolUpgrade(ProtocolUpgradeError),
    #[error("Unsupported deploy item variant: {0}")]
    InvalidDeployItemVariant(String),
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

impl From<GenesisError> for Error {
    fn from(genesis_error: GenesisError) -> Self {
        Self::Genesis(Box::new(genesis_error))
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
