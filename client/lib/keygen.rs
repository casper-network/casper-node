//! Cryptographic key generation.

use std::{fs, path::Path};

use casper_node::crypto::AsymmetricKeyExt;
use casper_types::{AsymmetricType, PublicKey, SecretKey};

use crate::error::{Error, Result};

/// Default filename for the PEM-encoded secret key file.
pub const SECRET_KEY_PEM: &str = "secret_key.pem";
/// Default filename for the hex-encoded public key file.
pub const PUBLIC_KEY_HEX: &str = "public_key_hex";
/// Default filename for the PEM-encoded public key file.
pub const PUBLIC_KEY_PEM: &str = "public_key.pem";

/// List of keygen related filenames: "secret_key.pem", "public_key.pem" and "public_key_hex".
pub const FILES: [&str; 3] = [SECRET_KEY_PEM, PUBLIC_KEY_PEM, PUBLIC_KEY_HEX];

/// Name of Ed25519 algorithm.
pub const ED25519: &str = "Ed25519";
/// Name of secp256k1 algorithm.
pub const SECP256K1: &str = "secp256k1";

/// Generates a new asymmetric key pair using the specified algorithm, and writes them to files in
/// the specified directory.
///
/// The secret key is written to "secret_key.pem", and the public key is written to "public_key.pem"
/// and also in hex format to "public_key_hex". For the hex format, the algorithm's tag is
/// prepended, e.g. `01` for Ed25519, `02` for secp256k1.
///
/// If `force` is true, existing files will be overwritten. If `force` is false and any of the
/// files exist, [`Error::FileAlreadyExists`](../enum.Error.html#variant.FileAlreadyExists) is
/// returned and no files are written.
pub fn generate_files(output_dir: &str, algorithm: &str, force: bool) -> Result<()> {
    if output_dir.is_empty() {
        return Err(Error::InvalidArgument {
            context: "generate_files",
            error: "empty output_dir provided, must be a valid path".to_string(),
        });
    }
    let _ = fs::create_dir_all(output_dir).map_err(move |error| Error::IoError {
        context: format!("unable to create directory at '{}'", output_dir),
        error,
    })?;
    let output_dir = Path::new(output_dir)
        .canonicalize()
        .map_err(|error| Error::IoError {
            context: format!("unable get canonical path at '{}'", output_dir),
            error,
        })?;

    if !force {
        for file in FILES.iter().map(|filename| output_dir.join(filename)) {
            if file.exists() {
                return Err(Error::FileAlreadyExists(file));
            }
        }
    }

    let secret_key = if algorithm.eq_ignore_ascii_case(ED25519) {
        SecretKey::generate_ed25519().unwrap()
    } else if algorithm.eq_ignore_ascii_case(SECP256K1) {
        SecretKey::generate_secp256k1().unwrap()
    } else {
        return Err(Error::UnsupportedAlgorithm(algorithm.to_string()));
    };

    let public_key = PublicKey::from(&secret_key);

    let public_key_hex_path = output_dir.join(PUBLIC_KEY_HEX);
    fs::write(public_key_hex_path, public_key.to_hex()).map_err(|error| Error::IoError {
        context: format!(
            "unable to write public key hex file at {:?}",
            output_dir.join(PUBLIC_KEY_HEX)
        ),
        error,
    })?;

    let secret_key_path = output_dir.join(SECRET_KEY_PEM);
    secret_key
        .to_file(&secret_key_path)
        .map_err(|error| Error::CryptoError {
            context: "secret_key",
            error,
        })?;

    let public_key_path = output_dir.join(PUBLIC_KEY_PEM);
    public_key
        .to_file(&public_key_path)
        .map_err(|error| Error::CryptoError {
            context: "public_key",
            error,
        })?;

    Ok(())
}
