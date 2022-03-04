use std::{env, fs, io, path::PathBuf, sync::Arc};

use serde::{Deserialize, Serialize};
use thiserror::Error;
use toml::de::Error as TomlDecodeError;
use tracing::info;

use casper_hashing::Digest;
use casper_types::{ProtocolVersion, PublicKey, SecretKey, Signature};

use crate::{
    crypto,
    reactor::participating::Config,
    types::{chainspec, Chainspec, ChainspecRawBytes},
    utils::{LoadError, Loadable, WithDir},
};

/// The name of the file for recording the new global state hash after a data migration.
const POST_MIGRATION_STATE_HASH_FILENAME: &str = "post-migration-state-hash";
/// The folder under which the post-migration-state-hash file is written.
const CONFIG_ROOT_DIR: &str = "/etc/casper";
/// Environment variable to override the config root dir.
const CONFIG_ROOT_DIR_OVERRIDE: &str = "CASPER_CONFIG_DIR";

// TODO - remove once used.
#[allow(unused)]
/// Error returned as a result of migrating data.
#[derive(Debug, Error)]
pub(crate) enum Error {
    /// Error serializing state hash info.
    #[error("error serializing state hash info: {0}")]
    SerializeStateHashInfo(bincode::Error),

    /// Error deserializing state hash info.
    #[error("error deserializing state hash info: {0}")]
    DeserializeStateHashInfo(bincode::Error),

    /// Error writing state hash info file.
    #[error("error writing state hash info to {path}: {error}")]
    WriteStateHashInfo {
        /// The file path.
        path: String,
        /// The IO error.
        error: io::Error,
    },

    /// Error reading state hash info file.
    #[error("error reading state hash info from {path}: {error}")]
    ReadStateHashInfo {
        /// The file path.
        path: String,
        /// The IO error.
        error: io::Error,
    },

    /// Invalid signature of state hash and version.
    #[error("invalid signature of state hash info")]
    InvalidSignatureOfStateHashInfo,

    /// Error reading config file.
    #[error("error reading config from {path}: {error}")]
    ReadConfig {
        /// The file path.
        path: String,
        /// The IO error.
        error: io::Error,
    },

    /// Error decoding config file.
    #[error("error reading config from {path}: {error}")]
    DecodeConfig {
        /// The file path.
        path: String,
        /// The TOML error.
        error: TomlDecodeError,
    },

    /// Error loading the secret key.
    #[error("error loading secret key: {0}")]
    LoadSecretKey(LoadError<crypto::Error>),

    /// Error loading the chainspec.
    #[error("error loading chainspec: {0}")]
    LoadChainspec(chainspec::Error),
}

#[derive(Serialize, Deserialize)]
struct PostMigrationInfo {
    state_hash: Digest,
    protocol_version: ProtocolVersion,
}

#[derive(Serialize, Deserialize)]
struct SignedPostMigrationInfo {
    serialized_info: Vec<u8>,
    signature: Signature,
}

/// Reads in the root hash of the global state after a previous run of data migration.
///
/// Returns `Ok(None)` if there is no saved file or if it doesn't contain the same version as
/// `protocol_version`.  Returns `Ok(Some)` if the file can be read and it contains the same version
/// as `protocol_version`.  Otherwise returns an error.
// TODO - remove once used.
#[allow(unused)]
pub(crate) fn read_post_migration_info(
    protocol_version: ProtocolVersion,
    public_key: &PublicKey,
) -> Result<Option<Digest>, Error> {
    do_read_post_migration_info(protocol_version, public_key, info_path())
}

// TODO - remove once used.
#[allow(unused)]
fn do_read_post_migration_info(
    protocol_version: ProtocolVersion,
    public_key: &PublicKey,
    path: PathBuf,
) -> Result<Option<Digest>, Error> {
    // If the file doesn't exist, return `Ok(None)`.
    if !path.is_file() {
        return Ok(None);
    }

    // Read the signed info.
    let serialized_signed_info = fs::read(&path).map_err(|error| Error::ReadStateHashInfo {
        path: path.display().to_string(),
        error,
    })?;
    let signed_info: SignedPostMigrationInfo =
        bincode::deserialize(&serialized_signed_info).map_err(Error::DeserializeStateHashInfo)?;

    // Validate the signature.
    crypto::verify(
        &signed_info.serialized_info,
        &signed_info.signature,
        public_key,
    )
    .map_err(|_| Error::InvalidSignatureOfStateHashInfo)?;

    // Deserialize the info.
    let info: PostMigrationInfo = bincode::deserialize(&signed_info.serialized_info)
        .map_err(Error::DeserializeStateHashInfo)?;

    if info.protocol_version == protocol_version {
        Ok(Some(info.state_hash))
    } else {
        Ok(None)
    }
}

/// Writes the root hash of the global state and the new protocol version after data migration has
/// completed.
///
/// This must be called after a data migration in order to allow the node to read in the new root
/// state on restart.
fn write_post_migration_info(
    state_hash: Digest,
    new_protocol_version: ProtocolVersion,
    secret_key: &SecretKey,
    path: PathBuf,
) -> Result<(), Error> {
    // Serialize the info.
    let info = PostMigrationInfo {
        state_hash,
        protocol_version: new_protocol_version,
    };
    let serialized_info = bincode::serialize(&info).map_err(Error::SerializeStateHashInfo)?;

    // Sign the info.
    let public_key = PublicKey::from(secret_key);
    let signature = crypto::sign(&serialized_info, secret_key, &public_key);
    let signed_info = SignedPostMigrationInfo {
        serialized_info,
        signature,
    };

    // Write the signed info to disk.
    let serialized_signed_info =
        bincode::serialize(&signed_info).map_err(Error::SerializeStateHashInfo)?;
    fs::write(&path, serialized_signed_info).map_err(|error| Error::WriteStateHashInfo {
        path: path.display().to_string(),
        error,
    })?;

    info!(path=%path.display(), "wrote post-migration state hash");
    Ok(())
}

fn info_path() -> PathBuf {
    PathBuf::from(
        env::var(CONFIG_ROOT_DIR_OVERRIDE).unwrap_or_else(|_| CONFIG_ROOT_DIR.to_string()),
    )
    .join(POST_MIGRATION_STATE_HASH_FILENAME)
}

/// Migrates data from that specified in the old config file to that specified in the new one.
pub(crate) fn migrate_data(
    _old_config: WithDir<toml::Value>,
    new_config: WithDir<Config>,
) -> Result<(), Error> {
    let (new_root, new_config) = new_config.into_parts();
    let new_protocol_version = <(Chainspec, ChainspecRawBytes)>::from_path(&new_root)
        .map_err(Error::LoadChainspec)?
        .0
        .protocol_config
        .version;
    let secret_key: Arc<SecretKey> = new_config
        .consensus
        .secret_key_path
        .load(&new_root)
        .map_err(Error::LoadSecretKey)?;

    // Get this by actually migrating the global state data.
    let state_hash = Digest::default();

    if state_hash != Digest::default() {
        write_post_migration_info(state_hash, new_protocol_version, &secret_key, info_path())?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rand::Rng;

    use super::*;
    use crate::crypto::AsymmetricKeyExt;

    #[test]
    fn should_write_then_read_info() {
        let tempdir = tempfile::tempdir().unwrap();
        let info_path = tempdir.path().join(POST_MIGRATION_STATE_HASH_FILENAME);

        let mut rng = crate::new_rng();
        let state_hash = Digest::hash(&[rng.gen()]);
        let protocol_version = ProtocolVersion::from_parts(rng.gen(), rng.gen(), rng.gen());
        let secret_key = SecretKey::random(&mut rng);

        write_post_migration_info(state_hash, protocol_version, &secret_key, info_path.clone())
            .unwrap();

        let public_key = PublicKey::from(&secret_key);
        let maybe_hash =
            do_read_post_migration_info(protocol_version, &public_key, info_path).unwrap();
        assert_eq!(maybe_hash, Some(state_hash));
    }

    #[test]
    fn should_return_none_after_reading_info() {
        let tempdir = tempfile::tempdir().unwrap();
        let info_path = tempdir.path().join(POST_MIGRATION_STATE_HASH_FILENAME);

        // Should return `None` if there is no info file.
        let protocol_version = ProtocolVersion::from_parts(1, 2, 3);
        let mut rng = crate::new_rng();
        let secret_key = SecretKey::random(&mut rng);
        let public_key = PublicKey::from(&secret_key);
        let maybe_hash =
            do_read_post_migration_info(protocol_version, &public_key, info_path.clone()).unwrap();
        assert!(maybe_hash.is_none());

        // Create the info file and check we can read it.
        let state_hash = Digest::hash(&[rng.gen()]);
        write_post_migration_info(state_hash, protocol_version, &secret_key, info_path.clone())
            .unwrap();
        assert!(
            do_read_post_migration_info(protocol_version, &public_key, info_path.clone())
                .unwrap()
                .is_some()
        );

        // Should return `None` for a version different to that requested.
        let different_version = ProtocolVersion::from_parts(1, 2, 4);
        let maybe_hash =
            do_read_post_migration_info(different_version, &public_key, info_path).unwrap();
        assert!(maybe_hash.is_none());
    }

    #[test]
    fn should_fail_to_read_invalid_info() {
        let tempdir = tempfile::tempdir().unwrap();
        let info_path = tempdir.path().join(POST_MIGRATION_STATE_HASH_FILENAME);

        // Should return `Err` if the file can't be parsed.
        fs::write(&info_path, "bad value".as_bytes()).unwrap();
        let protocol_version = ProtocolVersion::from_parts(1, 2, 3);
        let mut rng = crate::new_rng();
        let secret_key = SecretKey::random(&mut rng);
        let public_key = PublicKey::from(&secret_key);
        assert!(
            do_read_post_migration_info(protocol_version, &public_key, info_path.clone()).is_err()
        );

        // Should return `Err` if the signature is invalid.
        let other_secret_key = SecretKey::random(&mut rng);
        let state_hash = Digest::hash(&[rng.gen()]);
        write_post_migration_info(
            state_hash,
            protocol_version,
            &other_secret_key,
            info_path.clone(),
        )
        .unwrap();
        assert!(do_read_post_migration_info(protocol_version, &public_key, info_path).is_err());
    }
}
