//! Configuration file management
//!
//! Configuration for the node is loaded from TOML files, but all configuration values have
//! sensible defaults.
//!
//! The `cli` offers an option to generate a configuration from defaults for editing.
//!
//! # Adding a configuration section
//!
//! When adding a section to the configuration, ensure that
//!
//! * it has an entry in the root configuration `Config`,
//! * `Default` is implemented (derived or manually) with sensible defaults, and
//! * it is completely documented.

use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{fs, io, net, path};
use tracing::debug;

/// Root configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    /// Log configuration.
    pub log: Log,
    /// Network configuration for the validator-only network.
    pub validator_net: SmallNetwork,
    /// Network configuration for the public network.
    pub public_net: SmallNetwork,
}

/// Log configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct Log {
    /// Log level.
    #[serde(with = "log_level")]
    pub level: tracing::Level,
}

#[derive(Debug, Deserialize, Serialize)]
/// Small network configuration
pub struct SmallNetwork {
    /// Interface to bind to. If it is the same as the in `root_addr`, attempt
    /// become the root node for this particular small network.
    pub bind_interface: net::IpAddr,

    /// Port to bind to when not the root node. Use 0 for a random port.
    pub bind_port: u16,

    /// Address to connect to join the network.
    pub root_addr: net::SocketAddr,

    /// Path to certificate file.
    pub cert: Option<path::PathBuf>,

    /// Path to private key for certificate.
    pub private_key: Option<path::PathBuf>,
}

impl SmallNetwork {
    /// Create a default instance for `SmallNetwork` with a constant port.
    fn default_on_port(port: u16) -> Self {
        SmallNetwork {
            bind_interface: net::Ipv4Addr::new(127, 0, 0, 1).into(),
            bind_port: 0,
            root_addr: (net::Ipv4Addr::new(127, 0, 0, 1), port).into(),
            cert: None,
            private_key: None,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Config {
            log: Default::default(),
            validator_net: SmallNetwork::default_on_port(34553),
            public_net: SmallNetwork::default_on_port(1485),
        }
    }
}

impl Default for Log {
    fn default() -> Self {
        Log {
            level: tracing::Level::INFO,
        }
    }
}

impl Log {
    /// Initialize logging system based on settings in configuration.
    ///
    /// Will setup logging as described in this configuration for the whole application. This
    /// function should only be called once during the lifetime of the application.
    pub fn setup_logging(&self) -> anyhow::Result<()> {
        // Setup a new tracing-subscriber writing to `stderr` for logging.
        tracing::subscriber::set_global_default(
            tracing_subscriber::fmt()
                .with_writer(io::stderr)
                .with_max_level(self.level.clone())
                .finish(),
        )?;
        debug!("debug output enabled");

        Ok(())
    }
}

/// Loads a TOML-formatted configuration from a given file.
pub fn load_from_file<P: AsRef<path::Path>>(config_path: P) -> anyhow::Result<Config> {
    let path_ref = config_path.as_ref();
    Ok(toml::from_str(
        &fs::read_to_string(path_ref)
            .with_context(|| format!("Failed to read configuration file {:?}", path_ref))?,
    )
    .with_context(|| format!("Failed to parse configuration file {:?}", path_ref))?)
}

/// Create a TOML-formatted string from a given configuration.
pub fn to_string(cfg: &Config) -> anyhow::Result<String> {
    toml::to_string_pretty(cfg).with_context(|| "Failed to serialize default configuration")
}

/// Serialization/deserialization
mod log_level {
    use serde::{self, Deserialize};
    use std::str::FromStr;
    use tracing::Level;

    pub fn serialize<S>(value: &Level, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(value.to_string().as_str())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Level, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Level::from_str(s.as_str()).map_err(serde::de::Error::custom)
    }
}
