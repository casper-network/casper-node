use std::{
    fs,
    path::{Path, PathBuf},
};

use casper_node::crypto::asymmetric_key::{PublicKey, SecretKey};

use crate::error::Result;

pub const PUBLIC_KEY_HEX: &str = "public_key_hex";
pub const SECRET_KEY_PEM: &str = "secret_key.pem";
pub const PUBLIC_KEY_PEM: &str = "public_key.pem";
pub const FILES: [&str; 3] = [PUBLIC_KEY_HEX, SECRET_KEY_PEM, PUBLIC_KEY_PEM];

pub fn write_to_file(output_dir: PathBuf, secret_key: SecretKey) -> Result<()> {
    let public_key = PublicKey::from(&secret_key);
    write_file(PUBLIC_KEY_HEX, output_dir.as_path(), public_key.to_hex())?;
    let secret_key_path = output_dir.join(SECRET_KEY_PEM);
    secret_key.to_file(&secret_key_path)?;
    let public_key_path = output_dir.join(PUBLIC_KEY_PEM);
    public_key.to_file(&public_key_path)?;
    Ok(())
}

pub fn write_file(filename: &str, dir: &Path, value: String) -> Result<()> {
    let path = dir.join(filename);
    Ok(fs::write(&path, value)?)
}
