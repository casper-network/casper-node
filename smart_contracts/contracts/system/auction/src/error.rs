use core::result::Result as StdResult;

use casperlabs_types::{bytesrepr, ApiError};

#[repr(u16)]
#[cfg_attr(test, derive(Debug))]
pub enum Error {
    TargetBidDoesNotExists,
    MissingKey,
    InvalidKeyVariant,
    MissingValue,
    Serialization,
    Transfer,
}
impl From<bytesrepr::Error> for Error {
    fn from(_: bytesrepr::Error) -> Self {
        Error::Serialization
    }
}

impl From<Error> for ApiError {
    fn from(error: Error) -> Self {
        ApiError::User(error as u16)
    }
}

pub type Result<T> = StdResult<T, Error>;
