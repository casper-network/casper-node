use failure::Fail;
use parity_wasm::elements;

use crate::contract_shared::{wasm_prep, TypeMismatch};
use types::{
    account::{AddKeyFailure, RemoveKeyFailure, SetThresholdFailure, UpdateKeyFailure},
    bytesrepr, system_contract_errors, AccessRights, ApiError, CLType, CLValueError,
    ContractPackageHash, ContractVersionKey, Key, URef,
};

use crate::contract_core::resolvers::error::ResolverError;
use crate::contract_storage;

#[derive(Fail, Debug, Clone)]
pub enum Error {
    #[fail(display = "Interpreter error: {}", _0)]
    Interpreter(String),
    #[fail(display = "Storage error: {}", _0)]
    Storage(contract_storage::error::Error),
    #[fail(display = "Serialization error: {}", _0)]
    BytesRepr(bytesrepr::Error),
    #[fail(display = "Named key {} not found", _0)]
    NamedKeyNotFound(String),
    #[fail(display = "Key {} not found", _0)]
    KeyNotFound(Key),
    #[fail(display = "Account {:?} not found", _0)]
    AccountNotFound(Key),
    #[fail(display = "{}", _0)]
    TypeMismatch(TypeMismatch),
    #[fail(display = "Invalid access rights: {}", required)]
    InvalidAccess { required: AccessRights },
    #[fail(display = "Forged reference: {}", _0)]
    ForgedReference(URef),
    #[fail(display = "URef not found: {}", _0)]
    URefNotFound(String),
    #[fail(display = "Function not found: {}", _0)]
    FunctionNotFound(String),
    #[fail(display = "{}", _0)]
    ParityWasm(elements::Error),
    #[fail(display = "Out of gas error")]
    GasLimit,
    #[fail(display = "Return")]
    Ret(Vec<URef>),
    #[fail(display = "{}", _0)]
    Rng(String),
    #[fail(display = "Resolver error: {}", _0)]
    Resolver(ResolverError),
    /// Reverts execution with a provided status
    #[fail(display = "{}", _0)]
    Revert(ApiError),
    #[fail(display = "{}", _0)]
    AddKeyFailure(AddKeyFailure),
    #[fail(display = "{}", _0)]
    RemoveKeyFailure(RemoveKeyFailure),
    #[fail(display = "{}", _0)]
    UpdateKeyFailure(UpdateKeyFailure),
    #[fail(display = "{}", _0)]
    SetThresholdFailure(SetThresholdFailure),
    #[fail(display = "{}", _0)]
    SystemContract(system_contract_errors::Error),
    #[fail(display = "Deployment authorization failure")]
    DeploymentAuthorizationFailure,
    #[fail(display = "Expected return value")]
    ExpectedReturnValue,
    #[fail(display = "Unexpected return value")]
    UnexpectedReturnValue,
    #[fail(display = "Invalid context")]
    InvalidContext,
    #[fail(
        display = "Incompatible protocol major version. Expected version {} but actual version is {}",
        expected, actual
    )]
    IncompatibleProtocolMajorVersion { expected: u32, actual: u32 },
    #[fail(display = "{}", _0)]
    CLValue(CLValueError),
    #[fail(display = "Host buffer is empty")]
    HostBufferEmpty,
    #[fail(display = "Unsupported WASM start")]
    UnsupportedWasmStart,
    #[fail(display = "No active contract versions for contract package")]
    NoActiveContractVersions(ContractPackageHash),
    #[fail(display = "Invalid contract version: {}", _0)]
    InvalidContractVersion(ContractVersionKey),
    #[fail(display = "No such method: {}", _0)]
    NoSuchMethod(String),
    #[fail(display = "Wasm preprocessing error: {}", _0)]
    WasmPreprocessing(wasm_prep::PreprocessingError),
    #[fail(
        display = "Unexpected Key length. Expected length {} but actual length is {}",
        expected, actual
    )]
    InvalidKeyLength { expected: usize, actual: usize },
}

impl From<wasm_prep::PreprocessingError> for Error {
    fn from(error: wasm_prep::PreprocessingError) -> Self {
        Error::WasmPreprocessing(error)
    }
}

impl Error {
    pub fn type_mismatch(expected: CLType, found: CLType) -> Error {
        Error::TypeMismatch(TypeMismatch {
            expected: format!("{:?}", expected),
            found: format!("{:?}", found),
        })
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

impl From<contract_storage::error::Error> for Error {
    fn from(e: contract_storage::error::Error) -> Self {
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

impl From<system_contract_errors::Error> for Error {
    fn from(error: system_contract_errors::Error) -> Self {
        Error::SystemContract(error)
    }
}

impl From<CLValueError> for Error {
    fn from(e: CLValueError) -> Self {
        Error::CLValue(e)
    }
}
