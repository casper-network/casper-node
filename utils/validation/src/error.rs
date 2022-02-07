use std::{io, path::PathBuf};

use thiserror::Error;

use casper_types::bytesrepr;

use crate::test_case;

#[derive(Error, Debug)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Deserialize(#[from] serde_json::Error),
    #[error("missing file stem in: {0}")]
    NoStem(PathBuf),
    #[error("unsupported file format at {0}")]
    UnsupportedFormat(PathBuf),
    #[error("file {0} lacks extension")]
    NoExtension(PathBuf),
    #[error("{0}")]
    Bytesrepr(bytesrepr::Error),
    #[error(transparent)]
    TestCase(#[from] test_case::Error),
}

impl From<bytesrepr::Error> for Error {
    fn from(error: bytesrepr::Error) -> Self {
        Error::Bytesrepr(error)
    }
}
