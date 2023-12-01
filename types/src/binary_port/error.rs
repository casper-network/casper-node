//! Binary port error.

use thiserror::Error;

#[derive(Debug, Clone, Error)]
#[repr(u8)]
pub enum Error {
    #[error("request executed correctly")]
    NoError = 0,
    #[error("data not found")]
    NotFound = 1,
}
