use casper_types::bytesrepr;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{0}")]
    Bytesrepr(bytesrepr::Error),
    #[error("data mismatch expected {} != actual {}", base16::encode_lower(&.expected), base16::encode_lower(&.actual))]
    DataMismatch { expected: Vec<u8>, actual: Vec<u8> },
    #[error("length mismatch expected {expected} != actual {actual}")]
    LengthMismatch { expected: usize, actual: usize },
}

impl From<bytesrepr::Error> for Error {
    fn from(error: bytesrepr::Error) -> Self {
        Error::Bytesrepr(error)
    }
}

pub trait TestCase {
    fn run_test(&self) -> Result<(), Error>;
}
