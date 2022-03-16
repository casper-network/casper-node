//! Execution error and supporting code.
use parity_wasm::elements;
use thiserror::Error;

use casper_types::{
    account::{AddKeyFailure, RemoveKeyFailure, SetThresholdFailure, UpdateKeyFailure},
    bytesrepr, system, AccessRights, ApiError, CLType, CLValueError, ContractHash,
    ContractPackageHash, ContractVersionKey, ContractWasmHash, Key, StoredValueTypeMismatch, URef,
};

use crate::{
    core::{resolvers::error::ResolverError, runtime::stack},
    shared::wasm_prep,
    storage,
};

/// Possible execution errors.
#[derive(Error, Debug, Clone)]
pub enum Error {
    /// WASM interpreter error.
    #[error("Interpreter error: {}", _0)]
    Interpreter(String),
    /// Storage error.
    #[error("Storage error: {}", _0)]
    Storage(storage::error::Error),
    /// Failed to (de)serialize bytes.
    #[error("Serialization error: {}", _0)]
    BytesRepr(bytesrepr::Error),
    /// Unable to find named key.
    #[error("Named key {} not found", _0)]
    NamedKeyNotFound(String),
    /// Unable to find a key.
    #[error("Key {} not found", _0)]
    KeyNotFound(Key),
    /// Unable to find an account.
    #[error("Account {:?} not found", _0)]
    AccountNotFound(Key),
    /// Type mismatch error.
    #[error("{}", _0)]
    TypeMismatch(StoredValueTypeMismatch),
    /// Invalid access.
    #[error("Invalid access rights: {}", required)]
    InvalidAccess {
        /// Required access rights of the operation.
        required: AccessRights,
    },
    /// Forged reference error.
    #[error("Forged reference: {}", _0)]
    ForgedReference(URef),
    /// Unable to find a [`URef`].
    #[error("URef not found: {}", _0)]
    URefNotFound(URef),
    /// Unable to find a function.
    #[error("Function not found: {}", _0)]
    FunctionNotFound(String),
    /// Parity WASM error.
    #[error("{}", _0)]
    ParityWasm(elements::Error),
    /// Error optimizing WASM.
    #[error("WASM optimizer error")]
    WasmOptimizer,
    /// Execution exceeded the gas limit.
    #[error("Out of gas error")]
    GasLimit,
    /// A stored smart contract incorrectly called a ret function.
    #[error("Return")]
    Ret(Vec<URef>),
    /// Error using WASM host function resolver.
    #[error("Resolver error: {}", _0)]
    Resolver(ResolverError),
    /// Reverts execution with a provided status
    #[error("{}", _0)]
    Revert(ApiError),
    /// Error adding an associated key.
    #[error("{}", _0)]
    AddKeyFailure(AddKeyFailure),
    /// Error removing an associated key.
    #[error("{}", _0)]
    RemoveKeyFailure(RemoveKeyFailure),
    /// Error updating an associated key.
    #[error("{}", _0)]
    UpdateKeyFailure(UpdateKeyFailure),
    /// Error setting threshold on associated key.
    #[error("{}", _0)]
    SetThresholdFailure(SetThresholdFailure),
    /// Error executing system contract.
    #[error("{}", _0)]
    SystemContract(system::Error),
    /// Weight of all used associated keys does not meet account's deploy threshold.
    #[error("Deployment authorization failure")]
    DeploymentAuthorizationFailure,
    /// Host buffer expected a value to be present.
    #[error("Expected return value")]
    ExpectedReturnValue,
    /// Host buffer was not expected to contain a value.
    #[error("Unexpected return value")]
    UnexpectedReturnValue,
    /// Error calling a host function in a wrong context.
    #[error("Invalid context")]
    InvalidContext,
    /// Unable to execute a deploy with invalid major protocol version.
    #[error("Incompatible protocol major version. Expected version {expected} but actual version is {actual}")]
    IncompatibleProtocolMajorVersion {
        /// Expected major version.
        expected: u32,
        /// Actual major version supplied.
        actual: u32,
    },
    /// Error converting a CLValue.
    #[error("{0}")]
    CLValue(CLValueError),
    /// Unable to access host buffer.
    #[error("Host buffer is empty")]
    HostBufferEmpty,
    /// WASM bytes contains an unsupported "start" section.
    #[error("Unsupported WASM start")]
    UnsupportedWasmStart,
    /// Contract package has no active contract versions.
    #[error("No active contract versions for contract package")]
    NoActiveContractVersions(ContractPackageHash),
    /// Invalid contract version supplied.
    #[error("Invalid contract version: {}", _0)]
    InvalidContractVersion(ContractVersionKey),
    /// Contract does not have specified entry point.
    #[error("No such method: {}", _0)]
    NoSuchMethod(String),
    /// Error processing WASM bytes.
    #[error("Wasm preprocessing error: {}", _0)]
    WasmPreprocessing(wasm_prep::PreprocessingError),
    /// Unable to convert a [`Key`] into an [`URef`].
    #[error("Key is not a URef: {}", _0)]
    KeyIsNotAURef(Key),
    /// Unexpected variant of a stored value.
    #[error("Unexpected variant of a stored value")]
    UnexpectedStoredValueVariant,
    /// Error upgrading a locked contract package.
    #[error("A locked contract cannot be upgraded")]
    LockedContract(ContractPackageHash),
    /// Unable to find a contract package by a specified hash address.
    #[error("Invalid contract package: {}", _0)]
    InvalidContractPackage(ContractPackageHash),
    /// Unable to find a contract by a specified hash address.
    #[error("Invalid contract: {}", _0)]
    InvalidContract(ContractHash),
    /// Unable to find the WASM bytes specified by a hash address.
    #[error("Invalid contract WASM: {}", _0)]
    InvalidContractWasm(ContractWasmHash),
    /// Error calling a smart contract with a missing argument.
    #[error("Missing argument: {name}")]
    MissingArgument {
        /// Name of the required argument.
        name: String,
    },
    /// Error writing a dictionary item key which exceeded maximum allowed length.
    #[error("Dictionary item key exceeded maximum length")]
    DictionaryItemKeyExceedsLength,
    /// Missing system contract registry.
    #[error("Missing system contract registry")]
    MissingSystemContractRegistry,
    /// Missing system contract hash.
    #[error("Missing system contract hash: {0}")]
    MissingSystemContractHash(String),
    /// An attempt to push to the runtime stack which is already at the maximum height.
    #[error("Runtime stack overflow")]
    RuntimeStackOverflow,
    /// An attempt to write a value to global state where its serialized size is too large.
    #[error("Value too large")]
    ValueTooLarge,
    /// The runtime stack is `None`.
    #[error("Runtime stack missing")]
    MissingRuntimeStack,
}

impl From<wasm_prep::PreprocessingError> for Error {
    fn from(error: wasm_prep::PreprocessingError) -> Self {
        Error::WasmPreprocessing(error)
    }
}

impl From<pwasm_utils::OptimizerError> for Error {
    fn from(_optimizer_error: pwasm_utils::OptimizerError) -> Self {
        Error::WasmOptimizer
    }
}

impl Error {
    /// Returns new type mismatch error.
    pub fn type_mismatch(expected: CLType, found: CLType) -> Error {
        Error::TypeMismatch(StoredValueTypeMismatch::new(
            format!("{:?}", expected),
            format!("{:?}", found),
        ))
    }
}

impl wasmi::HostError for Error {}

impl From<wasmi::Error> for Error {
    fn from(error: wasmi::Error) -> Self {
        match error
            .as_host_error()
            .and_then(|host_error| host_error.downcast_ref::<Error>())
        {
            Some(error) => error.clone(),
            None => Error::Interpreter(error.into()),
        }
    }
}

impl From<storage::error::Error> for Error {
    fn from(e: storage::error::Error) -> Self {
        Error::Storage(e)
    }
}

impl From<bytesrepr::Error> for Error {
    fn from(e: bytesrepr::Error) -> Self {
        Error::BytesRepr(e)
    }
}

impl From<elements::Error> for Error {
    fn from(e: elements::Error) -> Self {
        Error::ParityWasm(e)
    }
}

impl From<ResolverError> for Error {
    fn from(err: ResolverError) -> Self {
        Error::Resolver(err)
    }
}

impl From<AddKeyFailure> for Error {
    fn from(err: AddKeyFailure) -> Self {
        Error::AddKeyFailure(err)
    }
}

impl From<RemoveKeyFailure> for Error {
    fn from(err: RemoveKeyFailure) -> Self {
        Error::RemoveKeyFailure(err)
    }
}

impl From<UpdateKeyFailure> for Error {
    fn from(err: UpdateKeyFailure) -> Self {
        Error::UpdateKeyFailure(err)
    }
}

impl From<SetThresholdFailure> for Error {
    fn from(err: SetThresholdFailure) -> Self {
        Error::SetThresholdFailure(err)
    }
}

impl From<system::Error> for Error {
    fn from(error: system::Error) -> Self {
        Error::SystemContract(error)
    }
}

impl From<CLValueError> for Error {
    fn from(e: CLValueError) -> Self {
        Error::CLValue(e)
    }
}

impl From<stack::RuntimeStackOverflow> for Error {
    fn from(_: stack::RuntimeStackOverflow) -> Self {
        Error::RuntimeStackOverflow
    }
}
