use serde::{Deserialize, Serialize};

use crate::{crypto::asymmetric_key::SecretKey, utils::External};

/// Consensus configuration.
#[derive(Debug, Deserialize, Serialize, Default)]
// Disallow unknown fields to ensure config files and command-line overrides contain valid keys.
#[serde(deny_unknown_fields)]
pub struct Config {
    /// Path to secret key file.
    pub secret_key_path: External<SecretKey>,
}
