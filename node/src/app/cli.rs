//! Command-line option parsing.
//!
//! Most configuration is done via config files (see [`config`](../config/index.html) for details).

use std::{
    io,
    io::Write,
    path::{Path, PathBuf},
    str::FromStr,
};

use anyhow::bail;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use regex::Regex;
use structopt::StructOpt;
use toml::Value;
use tracing::info;

use crate::config;
use casperlabs_node::{
    logging,
    reactor::{initializer, validator, Runner},
    tls,
};

// Note: The docstring on `Cli` is the help shown when calling the binary with `--help`.
#[derive(Debug, StructOpt)]
/// CasperLabs blockchain node.
pub enum Cli {
    /// Generate a self-signed node certificate.
    GenerateCert {
        /// Output path base of the certificate. The certificate will be stored as
        /// `output.crt.pem`, while the key will be stored as `output.key.pem`.
        output: PathBuf,
    },
    /// Generate a configuration file from defaults and dump it to stdout.
    GenerateConfig {},

    /// Run the validator node.
    ///
    /// Loads the configuration values from the given configuration file or uses defaults if not
    /// given, then runs the reactor.
    Validator {
        #[structopt(short, long, env)]
        /// Path to configuration file.
        config: Option<PathBuf>,

        #[structopt(short = "C", long)]
        /// config entries (section, field, value)
        config_ext: Vec<ConfigExt>,
    },
}

#[derive(Debug)]
/// Command line extension to be applied to toml based config file values.
pub struct ConfigExt {
    section: String,
    key: String,
    value: String,
}

impl ConfigExt {
    /// Updates toml table with updated or extended kvp.
    fn update_toml_table(&self, toml_value: &mut toml::Value) -> Option<()> {
        let table = toml_value.as_table_mut()?;
        if !table.contains_key(&self.section) {
            table.insert(
                self.section.clone(),
                toml::Value::Table(toml::value::Table::new()),
            );
        }
        let val = parse_toml_value(&self.value);
        table[&self.section]
            .as_table_mut()?
            .insert(self.key.clone(), val);
        Some(())
    }
}

impl FromStr for ConfigExt {
    type Err = &'static str;

    /// Attempts to create a ConfigExt from a str patterned as
    /// `section.key=value`
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let re = Regex::new(r"^([^.]+)\.([^=]+)=(.+)$").unwrap();
        if let Some(captures) = re.captures(input) {
            Ok(ConfigExt {
                section: captures.get(1).expect("section").as_str().to_owned(),
                key: captures.get(2).expect("section").as_str().to_owned(),
                value: captures.get(3).expect("section").as_str().to_owned(),
            })
        } else {
            Err("could not parse config_ext (see README.md)")
        }
    }
}

/// Convenience function to parse values passed via command
/// line into appropriate `toml::Value` representations.
fn parse_toml_value(raw: &str) -> toml::Value {
    if let Ok(value) = i64::from_str(raw) {
        return toml::Value::Integer(value);
    }
    if let Ok(value) = bool::from_str(raw) {
        return toml::Value::Boolean(value);
    }
    toml::Value::String(raw.to_string())
}

/// Normalizes any config key ending with `_path` with a value that appears to be
/// a relative path, relative to the config file path.
fn normalize_relative_paths(config_path: PathBuf, config: &mut toml::Value) {
    let table = if let Value::Table(table) = config {
        table
    } else {
        return;
    };

    for (_section, inner) in table {
        let table = if let toml::Value::Table(table) = inner {
            table
        } else {
            continue;
        };

        for (k, v) in table {
            // skip any key that does not appear to be a file path.
            if !k.ends_with("_path") {
                continue;
            }
            if let toml::Value::String(maybe_path) = v {
                let path = Path::new(maybe_path);
                if path.is_relative() {
                    *maybe_path = config_path.join(path).display().to_string();
                }
            }
        }
    }
}

impl Cli {
    /// Executes selected CLI command.
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            Cli::GenerateCert { output } => {
                if output.file_name().is_none() {
                    bail!("not a valid output path");
                }

                let mut cert_path = output.clone();
                cert_path.set_extension("crt.pem");

                let mut key_path = output;
                key_path.set_extension("key.pem");

                let (cert, key) = tls::generate_node_cert()?;

                tls::save_cert(&cert, cert_path)?;
                tls::save_private_key(&key, key_path)?;
            }
            Cli::GenerateConfig {} => {
                let cfg_str = config::to_string(&validator::Config::default())?;
                io::stdout().write_all(cfg_str.as_bytes())?;
            }
            Cli::Validator { config, config_ext } => {
                logging::init()?;

                // We use a `ChaCha20Rng` for the production node. For one, we want to completely
                // eliminate any chance of runtime failures, regardless of how small (these
                // exist with `OsRng`). Additionally, we want to limit the number of syscalls for
                // performance reasons.
                let mut rng = ChaCha20Rng::from_entropy();

                info!("resolving configuration");
                // the app supports running without a config file, using default values
                let maybe_config: Option<validator::Config> =
                    config.as_ref().map(config::load_from_file).transpose()?;

                // get the toml table version of the config indicated from cli args,
                // or from a new defaulted config instance if one is not provided
                let mut config_table: toml::Value =
                    toml::from_str(&toml::to_string(&maybe_config.unwrap_or_default()).unwrap())
                        .unwrap();

                // if any command line overrides to the config values are passed, apply them
                for item in config_ext {
                    item.update_toml_table(&mut config_table);
                }

                // if a config file path to a toml file was provided, normalize relative paths in
                // the config to the config file's path.
                // If a config file path was not passed via cli and a default config instance is
                // being used instead, do not normalize paths.
                let maybe_root_path =
                    config.map(|p| p.parent().unwrap().to_path_buf().canonicalize().unwrap());

                if let Some(root_path) = maybe_root_path {
                    normalize_relative_paths(root_path, &mut config_table)
                }

                // create validator config, including any overridden or normalized values
                let validator_config = match config_table.try_into() {
                    Ok(validator_config) => validator_config,
                    Err(error) => {
                        bail!(error);
                    }
                };

                let mut runner =
                    Runner::<initializer::Reactor>::new(validator_config, &mut rng).await?;
                runner.run(&mut rng).await;

                info!("finished initialization");

                let initializer = runner.into_inner();
                if !initializer.stopped_successfully() {
                    bail!("failed to initialize successfully");
                }

                let mut runner = Runner::<validator::Reactor>::from(initializer).await?;
                runner.run(&mut rng).await;
            }
        }

        Ok(())
    }
}
