//! Command-line option parsing.
//!
//! Most configuration is done via config files (see [`config`](../config/index.html) for details).

pub mod arglang;

use std::{
    alloc::System,
    borrow::Cow,
    fs,
    path::{Path, PathBuf},
    println,
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::{self, bail, Context};
use prometheus::Registry;
use regex::Regex;
use stats_alloc::{StatsAlloc, INSTRUMENTED_SYSTEM};
use structopt::StructOpt;
use toml::{value::Table, Value};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

use casper_storage::block_store::{
    lmdb::LmdbBlockStore,
    types::{BlockHashHeightAndEra, StateStoreKey},
    BlockStoreProvider, DataReader,
};
use casper_types::{Chainspec, ChainspecRawBytes, TransactionHash};

use crate::{
    components::network::Identity as NetworkIdentity,
    logging,
    reactor::{main_reactor, Runner},
    setup_signal_hooks, storage,
    types::ExitCode,
    utils::{
        chain_specification::validate_chainspec, config_specification::validate_config, Loadable,
        WithDir,
    },
};

// We override the standard allocator to gather metrics and tune the allocator via the MALLOC_CONF
// env var.
#[global_allocator]
static ALLOC: &StatsAlloc<System> = &INSTRUMENTED_SYSTEM;

// Note: The docstring on `Cli` is the help shown when calling the binary with `--help`.
#[derive(Debug, StructOpt)]
#[structopt(version = crate::VERSION_STRING_COLOR.as_str())]
#[allow(rustdoc::invalid_html_tags)]
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
    /// Verify that a given config file can be parsed.
    ValidateConfig {
        /// Path to configuration file.
        config: PathBuf,
    },
    /// Rebuild the disk-backed block-store indexes from scratch and report timing statistics.
    ///
    /// Clears the block-height, switch-block-era-id, and transaction-hash index databases, then
    /// repopulates them by scanning every block header currently in storage, printing progress
    /// as it goes. Does not touch any other stored data.
    BuildIndexes {
        /// Path to the LMDB storage directory (the directory containing `storage.lmdb`), i.e.
        /// the configured `storage.path` joined with the network name subdirectory.
        lmdb_path: PathBuf,

        /// Upper bound (in bytes) for the LMDB memory map. Must be at least as large as the
        /// store's configured `storage.max_block_store_size` + `max_deploy_store_size` +
        /// `max_deploy_metadata_store_size` (summed), or opening the environment will fail.
        /// Defaults to the sum of those three settings' default values.
        #[structopt(long)]
        max_size: Option<usize>,
    },
    /// Look up a single raw entry in one of the disk-backed block-store indexes.
    ///
    /// Prints the value stored under `key` (or `None` if there is no entry) using `Debug`
    /// formatting. This reads the index itself and does not resolve the value any further: e.g.
    /// for `block_height_index_db` this prints the indexed block hash, not the block it
    /// identifies.
    ReadIndex {
        /// Path to the LMDB storage directory (the directory containing `storage.lmdb`), i.e.
        /// the configured `storage.path` joined with the network name subdirectory.
        lmdb_path: PathBuf,

        /// Which index database to read from.
        index: IndexName,

        /// The key to look up in `index`: a `u64` block height for `block_height_index_db`, a
        /// `u64` era id for `switch_block_era_id_index_db`, or a JSON-encoded `TransactionHash`
        /// (e.g. `{"Version1":"0101..."}`) for `transaction_hash_index_db`.
        key: String,

        /// Upper bound (in bytes) for the LMDB memory map. See `build-indexes --max-size` for
        /// details on the default.
        #[structopt(long)]
        max_size: Option<usize>,
    },
    /// Show the `completed_blocks` disjoint sequences in human-readable form.
    ///
    /// Reads the state-store entry storage uses to track which block heights it has complete
    /// data for, and prints it as a comma-separated list of inclusive `[high, low]` ranges (e.g.
    /// `[20, 15], [8, 4]` means heights 15-20 and 4-8 are complete but 9-14 and 0-3 are not).
    ReadCompletedBlocks {
        /// Path to the LMDB storage directory (the directory containing `storage.lmdb`), i.e.
        /// the configured `storage.path` joined with the network name subdirectory.
        lmdb_path: PathBuf,

        /// Upper bound (in bytes) for the LMDB memory map. See `build-indexes --max-size` for
        /// details on the default.
        #[structopt(long)]
        max_size: Option<usize>,
    },
}

/// One of the disk-backed indexes maintained by [`LmdbBlockStore::rebuild_indexes`], as named on
/// the `read-index` CLI command line by its database name.
#[derive(Debug, Clone, Copy)]
pub enum IndexName {
    /// `block_height_index_db`: keyed by `u64` block height.
    BlockHeight,
    /// `switch_block_era_id_index_db`: keyed by `u64` era id.
    SwitchBlockEraId,
    /// `transaction_hash_index_db`: keyed by `TransactionHash`.
    TransactionHash,
}

impl FromStr for IndexName {
    type Err = anyhow::Error;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        match input {
            "block_height_index_db" => Ok(IndexName::BlockHeight),
            "switch_block_era_id_index_db" => Ok(IndexName::SwitchBlockEraId),
            "transaction_hash_index_db" => Ok(IndexName::TransactionHash),
            other => bail!(
                "unknown index {:?}: expected one of `block_height_index_db`, \
                 `switch_block_era_id_index_db`, `transaction_hash_index_db`",
                other
            ),
        }
    }
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

                let mut reactor_config = Self::init(&config, config_ext)?;

                // We use a `ChaCha20Rng` for the production node. For one, we want to completely
                // eliminate any chance of runtime failures, regardless of how small (these
                // exist with `OsRng`). Additionally, we want to limit the number of syscalls for
                // performance reasons.
                let mut rng = crate::new_rng();

                let registry = Registry::new();

                let (chainspec, chainspec_raw_bytes) =
                    <(Chainspec, ChainspecRawBytes)>::from_path(reactor_config.dir())?;

                info!(
                    protocol_version = %chainspec.protocol_version(),
                    build_version = %crate::VERSION_STRING.as_str(),
                    "node starting up"
                );

                if !validate_chainspec(&chainspec) {
                    bail!("invalid chainspec");
                }

                if !validate_config(reactor_config.value()) {
                    bail!("invalid config");
                }

                reactor_config.value_mut().ensure_valid(&chainspec);

                let network_identity = NetworkIdentity::from_config(WithDir::new(
                    reactor_config.dir(),
                    reactor_config.value().network.clone(),
                ))
                .context("failed to create a network identity")?;

                let mut main_runner = Runner::<main_reactor::MainReactor>::with_metrics(
                    reactor_config,
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
                    .map_or_else(|| "/".into(), Path::to_path_buf);
                let encoded_old_config = fs::read_to_string(&old_config)
                    .context("could not read old configuration file")
                    .with_context(|| old_config.display().to_string())?;
                let old_config = toml::from_str(&encoded_old_config)?;

                info!(build_version = %crate::VERSION_STRING.as_str(), "migrating config");
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
                    .map_or_else(|| "/".into(), Path::to_path_buf);
                let encoded_old_config = fs::read_to_string(&old_config)
                    .context("could not read old configuration file")
                    .with_context(|| old_config.display().to_string())?;
                let old_config = toml::from_str(&encoded_old_config)?;

                info!(build_version = %crate::VERSION_STRING.as_str(), "migrating data");
                crate::data_migration::migrate_data(
                    WithDir::new(old_root, old_config),
                    new_config,
                )?;
                Ok(ExitCode::Success as i32)
            }
            Cli::ValidateConfig { config } => {
                info!(build_version = %crate::VERSION_STRING.as_str(), config_file = ?config, "validating config file");
                match Self::init(&config, vec![]) {
                    Ok(_config) => {
                        info!(build_version = %crate::VERSION_STRING.as_str(), config_file = ?config, "config file is valid");
                        Ok(ExitCode::Success as i32)
                    }
                    Err(err) => {
                        // initialize manually in case of error to avoid double initialization
                        logging::init_with_config(&Default::default())?;
                        error!(build_version = %crate::VERSION_STRING.as_str(), config_file = ?config, "config file is not valid");
                        Err(err)
                    }
                }
            }
            Cli::BuildIndexes {
                lmdb_path,
                max_size,
            } => {
                logging::init_with_config(&Default::default())?;
                // The default filter only enables `info` level for the `casper_node` crate
                // itself; `LmdbBlockStore::rebuild_indexes` (in `casper_storage`) logs its
                // progress at `info` too, so widen the default here unless the operator set
                // their own `RUST_LOG`, otherwise this command runs silent.
                if std::env::var("RUST_LOG").is_err() {
                    logging::reload_global_env_filter(EnvFilter::new(
                        "warn,casper_node=info,casper_storage=info",
                    ))?;
                }

                // The default mirrors `storage::Config::default()`'s
                // `max_block_store_size + max_deploy_store_size + max_deploy_metadata_store_size`.
                let default_storage_config = storage::Config::default();
                let max_size = max_size.unwrap_or_else(|| {
                    default_storage_config.max_block_store_size
                        + default_storage_config.max_deploy_store_size
                        + default_storage_config.max_deploy_metadata_store_size
                });

                info!(
                    build_version = %crate::VERSION_STRING.as_str(),
                    path = %lmdb_path.display(),
                    "build-indexes: opening block store"
                );

                let mut block_store = LmdbBlockStore::new(&lmdb_path, max_size)?;

                info!("build-indexes: clearing and rebuilding disk-backed indexes");
                let start = Instant::now();
                let stats = block_store.rebuild_indexes()?;
                let elapsed = start.elapsed();

                let per_block = if stats.headers_processed > 0 {
                    elapsed / stats.headers_processed as u32
                } else {
                    Duration::ZERO
                };

                info!(
                    headers_processed = stats.headers_processed,
                    transactions_indexed = stats.transactions_indexed,
                    elapsed_secs = elapsed.as_secs_f64(),
                    per_block_micros = per_block.as_micros() as u64,
                    "build-indexes: summary"
                );
                println!(
                    "build-indexes: rebuilt indexes for {} block header(s) ({} transaction(s) \
                     indexed) in {:.3}s ({:.3} ms/block)",
                    stats.headers_processed,
                    stats.transactions_indexed,
                    elapsed.as_secs_f64(),
                    per_block.as_secs_f64() * 1000.0,
                );

                Ok(ExitCode::Success as i32)
            }
            Cli::ReadIndex {
                lmdb_path,
                index,
                key,
                max_size,
            } => {
                logging::init_with_config(&Default::default())?;

                // Mirrors `storage::Config::default()`'s summed store sizes; see the equivalent
                // comment on `Cli::BuildIndexes`.
                let default_storage_config = storage::Config::default();
                let max_size = max_size.unwrap_or_else(|| {
                    default_storage_config.max_block_store_size
                        + default_storage_config.max_deploy_store_size
                        + default_storage_config.max_deploy_metadata_store_size
                });

                let block_store = LmdbBlockStore::new(&lmdb_path, max_size)?;

                match index {
                    IndexName::BlockHeight => {
                        let height: u64 = key
                            .parse()
                            .with_context(|| format!("{:?} is not a valid u64 block height", key))?;
                        let value = block_store.read_block_height_index_entry(height)?;
                        println!("{:?}", value);
                    }
                    IndexName::SwitchBlockEraId => {
                        let era_id: u64 = key
                            .parse()
                            .with_context(|| format!("{:?} is not a valid u64 era id", key))?;
                        let value = block_store.read_switch_block_era_id_index_entry(era_id)?;
                        println!("{:?}", value);
                    }
                    IndexName::TransactionHash => {
                        let transaction_hash: TransactionHash =
                            serde_json::from_str(&key).with_context(|| {
                                format!("{:?} is not a valid JSON-encoded TransactionHash", key)
                            })?;
                        let value: Option<BlockHashHeightAndEra> =
                            block_store.checkout_ro()?.read(transaction_hash)?;
                        println!("{:?}", value);
                    }
                }

                Ok(ExitCode::Success as i32)
            }
            Cli::ReadCompletedBlocks {
                lmdb_path,
                max_size,
            } => {
                logging::init_with_config(&Default::default())?;

                // Mirrors `storage::Config::default()`'s summed store sizes; see the equivalent
                // comment on `Cli::BuildIndexes`.
                let default_storage_config = storage::Config::default();
                let max_size = max_size.unwrap_or_else(|| {
                    default_storage_config.max_block_store_size
                        + default_storage_config.max_deploy_store_size
                        + default_storage_config.max_deploy_metadata_store_size
                });

                let block_store = LmdbBlockStore::new(&lmdb_path, max_size)?;
                let maybe_raw: Option<Vec<u8>> = block_store.checkout_ro()?.read(
                    StateStoreKey::new(Cow::Borrowed(storage::COMPLETED_BLOCKS_STORAGE_KEY)),
                )?;

                match maybe_raw {
                    Some(raw) => {
                        let rendered = storage::disjoint_sequences::render(raw)
                            .context("failed to parse completed_blocks state-store entry")?;
                        println!("{}", rendered);
                    }
                    None => println!("no completed_blocks entry found"),
                }

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
            .map_or_else(|| "/".into(), Path::to_path_buf);

        // The app supports running without a config file, using default values.
        let encoded_config = fs::read_to_string(config)
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
