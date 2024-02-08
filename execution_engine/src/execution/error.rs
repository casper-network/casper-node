//! Execution error and supporting code.
use std::str::Utf8Error;
use thiserror::Error;

use casper_storage::{global_state, tracking_copy::TrackingCopyError};

use casper_types::{
    addressable_entity::{
        AddKeyFailure, EntityKind, RemoveKeyFailure, SetThresholdFailure, UpdateKeyFailure,
    },
    bytesrepr,
    execution::TransformError,
    system, AccessRights, AddressableEntityHash, ApiError, ByteCodeHash, CLType, CLValueError,
    EntityVersionKey, Key, PackageHash, StoredValueTypeMismatch, URef,
};
use casper_wasm::elements;

use crate::{
    resolvers::error::ResolverError,
    runtime::{stack, PreprocessingError},
};

/// Possible execution errors.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum Error {
    /// WASM interpreter error.
    #[error("Interpreter error: {}", _0)]
    Interpreter(String),
    /// Storage error.
    #[error("Storage error: {}", _0)]
    Storage(global_state::error::Error),
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
    /// A stored smart contract called a ret function.
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
    NoActiveEntityVersions(PackageHash),
    /// Invalid entity version supplied.
    #[error("Invalid entity version: {}", _0)]
    InvalidEntityVersion(EntityVersionKey),
    /// Contract does not have specified entry point.
    #[error("No such method: {}", _0)]
    NoSuchMethod(String),
    /// Contract does
    #[error("Error calling an abstract entry point: {}", _0)]
    TemplateMethod(String),
    /// Error processing WASM bytes.
    #[error("Wasm preprocessing error: {}", _0)]
    WasmPreprocessing(PreprocessingError),
    /// Unable to convert a [`Key`] into an [`URef`].
    #[error("Key is not a URef: {}", _0)]
    KeyIsNotAURef(Key),
    /// Unexpected variant of a stored value.
    #[error("Unexpected variant of a stored value")]
    UnexpectedStoredValueVariant,
    /// Error upgrading a locked contract package.
    #[error("A locked contract cannot be upgraded")]
    LockedEntity(PackageHash),
    /// Unable to find a contract package by a specified hash address.
    #[error("Invalid package: {}", _0)]
    InvalidPackage(PackageHash),
    /// Unable to find a contract by a specified hash address.
    #[error("Invalid contract: {}", _0)]
    InvalidEntity(AddressableEntityHash),
    /// Unable to find the WASM bytes specified by a hash address.
    #[error("Invalid contract WASM: {}", _0)]
    InvalidByteCode(ByteCodeHash),
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
    /// Contract is disabled.
    #[error("Contract is disabled")]
    DisabledEntity(AddressableEntityHash),
    /// Transform error.
    #[error(transparent)]
    Transform(TransformError),
    /// Invalid key
    #[error("Invalid key {0}")]
    UnexpectedKeyVariant(Key),
    /// Invalid AddressableEntity kind.
    #[error("Invalid entity kind: {0}")]
    InvalidEntityKind(EntityKind),
    /// Failed to transfer tokens on a private chain.
    #[error("Failed to transfer with unrestricted transfers disabled")]
    DisabledUnrestrictedTransfers,
    /// Storage error.
    #[error("Tracking copy error: {0}")]
    TrackingCopy(TrackingCopyError),
    /// Weight of all used associated keys does not meet entity's upgrade threshold.
    #[error("Deployment authorization failure")]
    UpgradeAuthorizationFailure,
    /// The EntryPoints contains an invalid entry.
    #[error("The EntryPoints contains an invalid entry")]
    InvalidEntryPointType,
    /// Invalid message topic operation.
    #[error("The requested operation is invalid for a message topic")]
    InvalidMessageTopicOperation,
    /// Invalid string encoding.
    #[error("Invalid UTF-8 string encoding: {0}")]
    InvalidUtf8Encoding(Utf8Error),
}

impl From<PreprocessingError> for Error {
    fn from(error: PreprocessingError) -> Self {
        Error::WasmPreprocessing(error)
    }
}

impl From<casper_wasm_utils::OptimizerError> for Error {
    fn from(_optimizer_error: casper_wasm_utils::OptimizerError) -> Self {
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

impl casper_wasmi::HostError for Error {}

impl From<casper_wasmi::Error> for Error {
    fn from(error: casper_wasmi::Error) -> Self {
        match error
            .as_host_error()
            .and_then(|host_error| host_error.downcast_ref::<Error>())
        {
            Some(error) => error.clone(),
            None => Error::Interpreter(error.into()),
        }
    }
}

impl From<global_state::error::Error> for Error {
    fn from(e: global_state::error::Error) -> Self {
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

impl From<TrackingCopyError> for Error {
    fn from(e: TrackingCopyError) -> Self {
        Error::TrackingCopy(e)
    }
}
