use core::result::Result as StdResult;

use types::{bytesrepr, ApiError, CLType, CLTyped};

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

impl Into<ApiError> for Error {
    fn into(self) -> ApiError {
        ApiError::User(self as u16)
    }
}

impl CLTyped for Error {
    fn cl_type() -> CLType {
        CLType::U32
    }
}

pub type Result<T> = StdResult<T, Error>;
