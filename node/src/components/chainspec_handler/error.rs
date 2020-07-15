use std::io::Error as IoError;

use thiserror::Error;
use uint::FromDecStrErr;

use types::account::ACCOUNT_HASH_LENGTH;

/// Error while encoding or decoding the chainspec.
#[derive(Debug, Error)]
pub enum Error {
    /// Error while encoding the chainspec to TOML format.
    #[error("encoding error: {0}")]
    EncodingToToml(#[from] toml::ser::Error),

    /// Error while decoding the chainspec from TOML format.
    #[error("decoding from TOML error: {0}")]
    DecodingFromToml(#[from] toml::de::Error),

    /// Error while decoding the genesis accounts from CSV format.
    #[error("decoding from CSV error: {0}")]
    DecodingFromCsv(#[from] csv::Error),

    /// Error while decoding a genesis account's key hash from hex format.
    #[error("decoding from hex error: {0}")]
    DecodingFromHex(#[from] hex::FromHexError),

    /// Error while decoding Motes from a decimal format.
    #[error("decoding motes from base-10 error: {0}")]
    DecodingMotes(#[from] FromDecStrErr),

    /// Error while decoding a genesis account's key hash from base-64 format.
    #[error("crypto module error: {0}")]
    Crypto(#[from] crate::crypto::Error),

    /// Decoding a genesis account's key hash yielded an invalid length byte array.
    #[error("expected hash length of {}, got {0}", ACCOUNT_HASH_LENGTH)]
    InvalidHashLength(usize),

    /// Error while trying to read in a config file.
    #[error("error reading {file}: {error}")]
    ReadFile {
        /// The file attempted to be read.
        file: String,
        /// The underlying error.
        error: IoError,
    },
}
