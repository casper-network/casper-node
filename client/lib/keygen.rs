//! Cryptographic key generation.

use std::{fmt::Display, fs, path::Path, result::Result as StdResult, str::FromStr};

use casper_node::crypto::asymmetric_key::{PublicKey, SecretKey};

use crate::error::{Error, Result};

/// Default filename for the hex-encoded public key file.
pub const PUBLIC_KEY_HEX: &str = "public_key_hex";
/// Default filename for the PEM-encoded secret key file.
pub const SECRET_KEY_PEM: &str = "secret_key.pem";
/// Default filename for the PEM-encoded public key file.
pub const PUBLIC_KEY_PEM: &str = "public_key.pem";

/// List of keygen related filenames.
pub const FILES: [&str; 3] = [PUBLIC_KEY_HEX, SECRET_KEY_PEM, PUBLIC_KEY_PEM];

/// Name of Ed25519 algorithm.
pub const ED25519: &str = "Ed25519";

/// Name of secp256k1 algorithm.
pub const SECP256K1: &str = "secp256k1";

/// Algorithm parameter for `keygen` module.
pub enum Algorithm {
    /// Ed25519 encryption algorithm.
    Ed25519,

    /// secp256k1 encryption algorithm.
    Secp256k1,
}

impl FromStr for Algorithm {
    type Err = String;

    fn from_str(strval: &str) -> StdResult<Self, Self::Err> {
        if strval.eq_ignore_ascii_case(ED25519) {
            Ok(Algorithm::Ed25519)
        } else if strval.eq_ignore_ascii_case(SECP256K1) {
            Ok(Algorithm::Secp256k1)
        } else {
            Err(format!("unsupported algorithm: {}", strval))
        }
    }
}

impl Display for Algorithm {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Algorithm::Ed25519 => write!(f, "{}", ED25519),
            Algorithm::Secp256k1 => write!(f, "{}", SECP256K1),
        }
    }
}

/// Writes related `PUBLIC_KEY_HEX`, `SECRET_KEY_PEM`, `PUBLIC_KEY_PEM` files to specified output
/// directory.
pub fn write_to_files(output_dir: &Path, secret_key: SecretKey) -> Result<()> {
    let public_key = PublicKey::from(&secret_key);
    write_file(PUBLIC_KEY_HEX, output_dir, public_key.to_hex())?;
    let secret_key_path = output_dir.join(SECRET_KEY_PEM);
    secret_key.to_file(&secret_key_path)?;
    let public_key_path = output_dir.join(PUBLIC_KEY_PEM);
    public_key.to_file(&public_key_path)?;
    Ok(())
}

fn write_file(filename: &str, dir: &Path, value: String) -> Result<()> {
    let path = dir.join(filename);
    Ok(fs::write(&path, value)?)
}

/// Writes related `PUBLIC_KEY_HEX`, `SECRET_KEY_PEM`, `PUBLIC_KEY_PEM` files to specified output
/// directory, generating a new secret key with the provided `algorithm`. Optionally `force`
/// overwrite of those files or raise an `Error::FileAlreadyExists(path)` if they already exist.
pub fn generate_files(output_dir: &Path, algorithm: Algorithm, force: bool) -> Result<()> {
    let _ = fs::create_dir_all(output_dir)?;
    let output_dir = output_dir.canonicalize()?;
    if !force {
        for file in FILES.iter().map(|filename| output_dir.join(filename)) {
            if file.exists() {
                return Err(Error::FileAlreadyExists(file));
            }
        }
    }
    let secret_key = match algorithm {
        Algorithm::Ed25519 => SecretKey::generate_ed25519(),
        Algorithm::Secp256k1 => SecretKey::generate_secp256k1(),
    };
    write_to_files(&output_dir, secret_key)
}
