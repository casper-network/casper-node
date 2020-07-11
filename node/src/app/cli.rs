//! Command-line option parsing.
//!
//! Most configuration is done via config files (see [`config`](../config/index.html) for details).

use std::{io, io::Write, path::PathBuf};

use anyhow::bail;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use structopt::StructOpt;
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
        #[structopt(short = "p", long, env)]
        /// Path to chainspec configuration file.
        chainspec_path: PathBuf,
        #[structopt(short, long, env)]
        /// Path to configuration file.
        config: Option<PathBuf>,
    },
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
            Cli::Validator {
                chainspec_path,
                config,
            } => {
                logging::init()?;

                // We use a `ChaCha20Rng` for the production node. For one, we want to completely
                // eliminate any chance of runtime failures, regardless of how small (these
                // exist with `OsRng`). Additionally, we want to limit the number of syscalls for
                // performance reasons.
                let mut rng = ChaCha20Rng::from_entropy();

                // We load the specified config, if any, otherwise use defaults.
                let config: validator::Config = config
                    .map(config::load_from_file)
                    .transpose()?
                    .unwrap_or_default();

                let mut runner =
                    Runner::<initializer::Reactor>::new((chainspec_path, config), &mut rng).await?;
                runner.run(&mut rng).await;

                info!("finished initialization");

                let initializer = runner.into_inner();
                if !initializer.stopped_successfully() {
                    bail!("failed to initialize successfully");
                }

                let mut runner = Runner::<validator::Reactor>::new(initializer, &mut rng).await?;
                runner.run(&mut rng).await;
            }
        }

        Ok(())
    }
}
