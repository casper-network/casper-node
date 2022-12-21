//! Command-line option parsing.
//!
//! Most configuration is done via config files (see [`config`](../config/index.html) for details).

pub mod arglang;

use std::{
    alloc::System,
    env, fs,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use anyhow::{self, bail, Context};
use prometheus::Registry;
use regex::Regex;
use stats_alloc::{StatsAlloc, INSTRUMENTED_SYSTEM};
use structopt::StructOpt;
use toml::{value::Table, Value};
use tracing::info;

use crate::{
    components::network::Identity as NetworkIdentity,
    logging,
    reactor::{main_reactor, Runner},
    setup_signal_hooks,
    types::{Chainspec, ChainspecRawBytes, ExitCode},
    utils::{Loadable, WithDir},
};

// We override the standard allocator to gather metrics and tune the allocator via th MALLOC_CONF
// env var.
#[global_allocator]
static ALLOC: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

// Note: The docstring on `Cli` is the help shown when calling the binary with `--help`.
#[derive(Debug, StructOpt)]
#[structopt(version = crate::VERSION_STRING_COLOR.as_str())]
/// Casper blockchain node.
pub enum Cli {
    /// Run the node in standard mode.
    ///
    /// Loads the configuration values from the given configuration file or uses defaults if not
    /// given, then runs the reactor.
    #[structopt(alias = "validator")]
    Standard {
        /// Path to configuration file.
        config: PathBuf,

        #[structopt(
            short = "C",
            long,
            env = "NODE_CONFIG",
            use_delimiter(true),
            value_delimiter(";")
        )]
        /// Overrides and extensions for configuration file entries in the form
        /// <SECTION>.<KEY>=<VALUE>.  For example, '-C=node.chainspec_config_path=chainspec.toml'
        config_ext: Vec<ConfigExt>,
    },
    /// Migrate modified values from the old config as required after an upgrade.
    MigrateConfig {
        /// Path to configuration file of previous version of node.
        #[structopt(long)]
        old_config: PathBuf,
        /// Path to configuration file of this version of node.
        #[structopt(long)]
        new_config: PathBuf,
    },
    /// Migrate any stored data as required after an upgrade.
    MigrateData {
        /// Path to configuration file of previous version of node.
        #[structopt(long)]
        old_config: PathBuf,
        /// Path to configuration file of this version of node.
        #[structopt(long)]
        new_config: PathBuf,
    },
}

#[derive(Debug)]
/// Command line extension to be applied to TOML-based config file values.
pub struct ConfigExt {
    section: String,
    key: String,
    value: String,
}

impl ConfigExt {
    /// Updates TOML table with updated or extended key value pairs.
    ///
    /// Returns errors if the respective sections to be updated are not TOML tables or if parsing
    /// the command line options failed.
    fn update_toml_table(&self, toml_value: &mut Value) -> anyhow::Result<()> {
        let table = toml_value
            .as_table_mut()
            .ok_or_else(|| anyhow::anyhow!("configuration table is not a table"))?;

        if !table.contains_key(&self.section) {
            table.insert(self.section.clone(), Value::Table(Table::new()));
        }
        let val = arglang::parse(&self.value)?;
        table[&self.section]
            .as_table_mut()
            .ok_or_else(|| {
                anyhow::anyhow!("configuration section {} is not a table", self.section)
            })?
            .insert(self.key.clone(), val);
        Ok(())
    }
}

impl FromStr for ConfigExt {
    type Err = anyhow::Error;

    /// Attempts to create a ConfigExt from a str patterned as `section.key=value`
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let re = Regex::new(r"^([^.]+)\.([^=]+)=(.+)$").unwrap();
        let captures = re
            .captures(input)
            .context("could not parse config_ext (see README.md)")?;
        Ok(ConfigExt {
            section: captures
                .get(1)
                .context("failed to find section")?
                .as_str()
                .to_owned(),
            key: captures
                .get(2)
                .context("failed to find key")?
                .as_str()
                .to_owned(),
            value: captures
                .get(3)
                .context("failed to find value")?
                .as_str()
                .to_owned(),
        })
    }
}

impl Cli {
    /// Executes selected CLI command.
    pub async fn run(self) -> anyhow::Result<i32> {
        match self {
            Cli::Standard { config, config_ext } => {
                // Setup UNIX signal hooks.
                setup_signal_hooks();

                let validator_config = Self::init(&config, config_ext)?;
                info!(version = %crate::VERSION_STRING.as_str(), "node starting up");

                // We use a `ChaCha20Rng` for the production node. For one, we want to completely
                // eliminate any chance of runtime failures, regardless of how small (these
                // exist with `OsRng`). Additionally, we want to limit the number of syscalls for
                // performance reasons.
                let mut rng = crate::new_rng();

                let registry = Registry::new();

                let (chainspec, chainspec_raw_bytes) =
                    <(Chainspec, ChainspecRawBytes)>::from_path(validator_config.dir())?;

                if !chainspec.is_valid() {
                    bail!("invalid chainspec");
                }

                let network_identity = NetworkIdentity::from_config(WithDir::new(
                    validator_config.dir(),
                    validator_config.value().network.clone(),
                ))
                .context("failed to create a network identity")?;

                let mut main_runner = Runner::<main_reactor::MainReactor>::with_metrics(
                    validator_config,
                    Arc::new(chainspec),
                    Arc::new(chainspec_raw_bytes),
                    network_identity,
                    &mut rng,
                    &registry,
                )
                .await?;

                let exit_code = main_runner.run(&mut rng).await;
                Ok(exit_code as i32)
            }
            Cli::MigrateConfig {
                old_config,
                new_config,
            } => {
                let new_config = Self::init(&new_config, vec![])?;

                let old_root = old_config
                    .parent()
                    .map(|path| path.to_owned())
                    .unwrap_or_else(|| "/".into());
                let encoded_old_config = fs::read_to_string(&old_config)
                    .context("could not read old configuration file")
                    .with_context(|| old_config.display().to_string())?;
                let old_config = toml::from_str(&encoded_old_config)?;

                info!(version = %env!("CARGO_PKG_VERSION"), "migrating config");
                crate::config_migration::migrate_config(
                    WithDir::new(old_root, old_config),
                    new_config,
                )?;
                Ok(ExitCode::Success as i32)
            }
            Cli::MigrateData {
                old_config,
                new_config,
            } => {
                let new_config = Self::init(&new_config, vec![])?;

                let old_root = old_config
                    .parent()
                    .map(|path| path.to_owned())
                    .unwrap_or_else(|| "/".into());
                let encoded_old_config = fs::read_to_string(&old_config)
                    .context("could not read old configuration file")
                    .with_context(|| old_config.display().to_string())?;
                let old_config = toml::from_str(&encoded_old_config)?;

                info!(version = %env!("CARGO_PKG_VERSION"), "migrating data");
                crate::data_migration::migrate_data(
                    WithDir::new(old_root, old_config),
                    new_config,
                )?;
                Ok(ExitCode::Success as i32)
            }
        }
    }

    /// Parses the config file for the current version of casper-node, and initializes logging.
    fn init(
        config: &Path,
        config_ext: Vec<ConfigExt>,
    ) -> anyhow::Result<WithDir<main_reactor::Config>> {
        // Determine the parent directory of the configuration file, if any.
        // Otherwise, we default to `/`.
        let root = config
            .parent()
            .map(|path| path.to_owned())
            .unwrap_or_else(|| "/".into());

        // The app supports running without a config file, using default values.
        let encoded_config = fs::read_to_string(&config)
            .context("could not read configuration file")
            .with_context(|| config.display().to_string())?;

        // Get the TOML table version of the config indicated from CLI args, or from a new
        // defaulted config instance if one is not provided.
        let mut config_table: Value = toml::from_str(&encoded_config)?;

        // If any command line overrides to the config values are passed, apply them.
        for item in config_ext {
            item.update_toml_table(&mut config_table)?;
        }

        // Create main config, including any overridden values.
        let main_config: main_reactor::Config = config_table.try_into()?;
        logging::init_with_config(&main_config.logging)?;

        Ok(WithDir::new(root, main_config))
    }
}
