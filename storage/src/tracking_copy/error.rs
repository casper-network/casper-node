use thiserror::Error;

use casper_types::{
    addressable_entity::{AddKeyFailure, RemoveKeyFailure, SetThresholdFailure, UpdateKeyFailure},
    bytesrepr, system, ApiError, CLType, CLValueError, Key, StoredValueTypeMismatch,
};

/// Possible tracking copy errors.
#[derive(Error, Debug, Clone)]
#[non_exhaustive]
pub enum Error {
    /// Storage error.
    #[error("Storage error: {}", _0)]
    Storage(crate::global_state::error::Error),
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
    /// ApiError.
    #[error("{}", _0)]
    Api(ApiError),
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
    /// Error converting a CLValue.
    #[error("{0}")]
    CLValue(CLValueError),
    /// Unexpected variant of a stored value.
    #[error("Unexpected variant of a stored value")]
    UnexpectedStoredValueVariant,
    /// Missing system contract hash.
    #[error("Missing system contract hash: {0}")]
    MissingSystemContractHash(String),
    /// Invalid key
    #[error("Invalid key {0}")]
    UnexpectedKeyVariant(Key),
    /// Circular reference error.
    #[error("Query attempted a circular reference: {0}")]
    CircularReference(String),
    /// Depth limit reached.
    #[error("Query exceeded depth limit: {depth}")]
    QueryDepthLimit {
        /// Current depth limit.
        depth: u64,
    },
    /// Missing bid.
    #[error("Missing bid: {0}")]
    MissingBid(Key),
    /// Not authorized.
    #[error("Authorization error")]
    Authorization,
    /// The value wasn't found.
    #[error("Value not found")]
    ValueNotFound(String),
    /// Unable to find a contract.
    #[error("Contract {:?} not found", _0)]
    ContractNotFound(Key),
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

impl From<bytesrepr::Error> for Error {
    fn from(e: bytesrepr::Error) -> Self {
        Error::BytesRepr(e)
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

impl From<CLValueError> for Error {
    fn from(e: CLValueError) -> Self {
        Error::CLValue(e)
    }
}

impl From<crate::global_state::error::Error> for Error {
    fn from(gse: crate::global_state::error::Error) -> Self {
        Error::Storage(gse)
    }
}
