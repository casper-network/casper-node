use std::{
    fs,
    path::{Path, PathBuf},
};

use casper_node::crypto::asymmetric_key::{PublicKey, SecretKey};

use crate::error::Result;

/// Default filename for the public key HEX file.
pub const PUBLIC_KEY_HEX: &str = "public_key_hex";
/// Default filename for the secret key file.
pub const SECRET_KEY_PEM: &str = "secret_key.pem";
/// Default filename for the public key PEM file.
pub const PUBLIC_KEY_PEM: &str = "public_key.pem";

/// List of keygen related filenames.
pub const FILES: [&str; 3] = [PUBLIC_KEY_HEX, SECRET_KEY_PEM, PUBLIC_KEY_PEM];

/// Write related PUBLIC_KEY_HEX, SECRET_KEY_PEM, PUBLIC_KEY_PEM files to specified output dir.
pub fn write_to_file(output_dir: PathBuf, secret_key: SecretKey) -> Result<()> {
    let public_key = PublicKey::from(&secret_key);
    write_file(PUBLIC_KEY_HEX, output_dir.as_path(), public_key.to_hex())?;
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
