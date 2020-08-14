use thiserror::Error;
use uint::FromDecStrErr;

use casperlabs_types::account::ACCOUNT_HASH_LENGTH;

use crate::utils::{LoadError, ReadFileError};

/// Error while encoding or decoding the chainspec.
#[derive(Debug, Error)]
pub enum Error {
    /// Error while encoding the chainspec to TOML format.
    #[error("encoding error: {0}")]
    EncodingToToml(#[from] toml::ser::Error),

    /// Error while decoding the chainspec from TOML format.
    #[error("decoding from TOML error: {0}")]
    DecodingFromToml(#[from] toml::de::Error),

    /// Error while decoding Motes from a decimal format.
    #[error("decoding motes from base-10 error: {0}")]
    DecodingMotes(#[from] FromDecStrErr),

    /// Error loading the upgrade installer.
    #[error("could not load upgrade installer: {0}")]
    LoadUpgradeInstaller(LoadError<ReadFileError>),

    /// Error loading the chainspec.
    #[error("could not load chainspec: {0}")]
    LoadChainspec(ReadFileError),

    /// Error loading the mint installer.
    #[error("could not load mint installer: {0}")]
    LoadMintInstaller(LoadError<ReadFileError>),

    /// Error loading the pos installer.
    #[error("could not load pos installer: {0}")]
    LoadPosInstaller(LoadError<ReadFileError>),

    /// Error loading the standard payment installer.
    #[error("could not load standard payment installer: {0}")]
    LoadStandardPaymentInstaller(LoadError<ReadFileError>),

    /// Error loading the standard payment installer.
    #[error("could not load genesis accounts installer: {0}")]
    LoadGenesisAccounts(LoadError<GenesisLoadError>),
}

/// Error loading genesis accounts file.
#[derive(Debug, Error)]
pub enum GenesisLoadError {
    /// Error while decoding the genesis accounts from CSV format.
    #[error("decoding from CSV error: {0}")]
    DecodingFromCsv(#[from] csv::Error),

    /// Error while decoding a genesis account's key hash from hex format.
    #[error("decoding from hex error: {0}")]
    DecodingFromHex(#[from] hex::FromHexError),

    /// Error while decoding Motes from a decimal format.
    #[error("decoding motes from base-10 error: {0}")]
    DecodingMotes(#[from] FromDecStrErr),

    /// Decoding a genesis account's key hash yielded an invalid length byte array.
    #[error("expected hash length of {}, got {0}", ACCOUNT_HASH_LENGTH)]
    InvalidHashLength(usize),

    /// Error while decoding a genesis account's key hash from base-64 format.
    #[error("crypto module error: {0}")]
    Crypto(#[from] crate::crypto::Error),
}
