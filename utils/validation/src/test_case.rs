use casper_types::bytesrepr;
use hex::FromHexError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Bytesrepr(bytesrepr::Error),
    #[error("data mismatch expected {} != actual {}", base16::encode_lower(&.expected), base16::encode_lower(&.actual))]
    DataMismatch { expected: Vec<u8>, actual: Vec<u8> },
    #[error("length mismatch expected {expected} != actual {actual}")]
    LengthMismatch { expected: usize, actual: usize },
    #[error("expected JSON string in output field")]
    WrongOutputType,
    #[error("not a valid hex string")]
    Hex(#[from] FromHexError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

impl From<bytesrepr::Error> for Error {
    fn from(error: bytesrepr::Error) -> Self {
        Error::Bytesrepr(error)
    }
}

pub trait TestCase {
    fn run_test(&self) -> Result<(), Error>;
}
