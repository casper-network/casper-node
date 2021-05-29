//! Command-line option parsing.
//!
//! Most configuration is done via config files (see [`config`](../config/index.html) for details).

pub mod arglang;

use std::{
    env, fs,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::{self, Context};
use regex::Regex;
use structopt::StructOpt;
use toml::{value::Table, Value};
use tracing::{debug, error, info, warn};

use casper_node::{
    logging,
    reactor::{initializer, joiner, validator, ReactorExit, Runner},
    setup_signal_hooks,
    types::ExitCode,
    utils::{
        pid_file::{PidFile, PidFileOutcome},
        WithDir,
    },
};
use prometheus::Registry;

// We override the standard allocator to gather metrics and tune the allocator via th MALLOC_CONF
// env var.
#[global_allocator]
static ALLOC: jemallocator::Jemalloc = jemallocator::Jemalloc;

// Note: The docstring on `Cli` is the help shown when calling the binary with `--help`.
#[derive(Debug, StructOpt)]
#[structopt(version = casper_node::VERSION_STRING_COLOR.as_str())]
/// Casper blockchain node.
pub enum Cli {
    /// Run the validator node.
    ///
    /// Loads the configuration values from the given configuration file or uses defaults if not
    /// given, then runs the reactor.
    Validator {
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
            Cli::Validator { config, config_ext } => {
                // Setup UNIX signal hooks.
                setup_signal_hooks();

                let validator_config = Self::init(&config, config_ext)?;
                info!(version = %casper_node::VERSION_STRING.as_str(), "node starting up");

                let pidfile_outcome = {
                    // Determine storage directory to store pidfile in.
                    let storage_config = validator_config.map_ref(|cfg| cfg.storage.clone());
                    let root = storage_config.with_dir(storage_config.value().path.clone());

                    // Create directory if it does not exist, similar to how the storage component
                    // would do it.
                    if !root.exists() {
                        fs::create_dir_all(&root).context("create storage directory")?;
                    }

                    PidFile::acquire(root.join("initializer.pid"))
                };

                // Note: Do not change `_pidfile` to `_`, or it will be dropped prematurely.
                // Instantiating `pidfile` guarantees that it will be dropped _after_ any reactor,
                // which is what we want.
                let (_pidfile, crashed) = match pidfile_outcome {
                    PidFileOutcome::AnotherNodeRunning(_) => {
                        anyhow::bail!("another node instance is running (pidfile is locked)");
                    }
                    PidFileOutcome::Crashed(pidfile) => {
                        warn!("previous node instance seems to have crashed, integrity checks may be run");
                        (pidfile, true)
                    }
                    PidFileOutcome::Clean(pidfile) => {
                        info!("no previous crash detected");
                        (pidfile, false)
                    }
                    PidFileOutcome::PidFileError(err) => {
                        return Err(anyhow::anyhow!(err));
                    }
                };

                // We use a `ChaCha20Rng` for the production node. For one, we want to completely
                // eliminate any chance of runtime failures, regardless of how small (these
                // exist with `OsRng`). Additionally, we want to limit the number of syscalls for
                // performance reasons.
                let mut rng = casper_node::new_rng();

                // The metrics are shared across all reactors.
                let registry = Registry::new();

                let mut initializer_runner = Runner::<initializer::Reactor>::with_metrics(
                    (crashed, validator_config),
                    &mut rng,
                    &registry,
                )
                .await?;

                // let mut initializer2_runner = Runner::<initializer2::Initializer>::with_metrics(
                //     WithDir::new(root.clone(), validator_config),
                //     &mut rng,
                //     &registry,
                // )
                // .await?;
                // initializer2_runner.run(&mut rng).await;

                match initializer_runner.run(&mut rng).await {
                    ReactorExit::ProcessShouldExit(exit_code) => return Ok(exit_code as i32),
                    ReactorExit::ProcessShouldContinue => info!("finished initialization"),
                }

                let initializer = initializer_runner.into_inner();
                let root = config
                    .parent()
                    .map(|path| path.to_owned())
                    .unwrap_or_else(|| "/".into());
                let mut joiner_runner = Runner::<joiner::Reactor>::with_metrics(
                    WithDir::new(root, initializer),
                    &mut rng,
                    &registry,
                )
                .await?;
                match joiner_runner.run(&mut rng).await {
                    ReactorExit::ProcessShouldExit(exit_code) => return Ok(exit_code as i32),
                    ReactorExit::ProcessShouldContinue => info!("finished joining"),
                }

                let (joiner_reactor, joiner_queue) = joiner_runner.into_inners();
                let config = joiner_reactor.into_validator_config().await?;

                // At this point, the joiner is shut down, so we clear the queue to ensure any
                // connections whose handshake completed but have not been registered are dropped.
                joiner_queue.seal();
                for event in joiner_queue.drain_queues().await {
                    debug!(event=%event, "drained event");
                }

                let mut validator_runner =
                    Runner::<validator::Reactor>::with_metrics(config, &mut rng, &registry).await?;

                match validator_runner.run(&mut rng).await {
                    ReactorExit::ProcessShouldExit(exit_code) => Ok(exit_code as i32),
                    reactor_exit => {
                        error!("validator should not exit with {:?}", reactor_exit);
                        Ok(ExitCode::Abort as i32)
                    }
                }
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
                casper_node::migrate_config(WithDir::new(old_root, old_config), new_config)?;
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
                casper_node::migrate_data(WithDir::new(old_root, old_config), new_config)?;
                Ok(ExitCode::Success as i32)
            }
        }
    }

    /// Parses the config file for the current version of casper-node, and initializes logging.
    fn init(
        config: &Path,
        config_ext: Vec<ConfigExt>,
    ) -> anyhow::Result<WithDir<validator::Config>> {
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

        // Create validator config, including any overridden values.
        let validator_config: validator::Config = config_table.try_into()?;
        logging::init_with_config(&validator_config.logging)?;

        Ok(WithDir::new(root, validator_config))
    }
}
