//! Command-line option parsing.
//!
//! Most configuration is done through the configuration, which is the only required command-line
//! argument. However some configuration values can be overwritten for convenience's sake.
use std::{io, io::Write, path};
use structopt::StructOpt;

use crate::{config, reactor, tls};

// Note: The docstring on `Cli` is the help shown when calling the binary with `--help`.
#[derive(Debug, StructOpt)]
/// CasperLabs blockchain node.
pub enum Cli {
    /// Generate a self-signed node certificate
    GenerateCert {
        /// Output path base the certificate, private key pair in PEM format. The cert will be stored as `output.crt.pem`, while the key will be stored as `output.key.pem`.
        output: path::PathBuf,
    },
    /// Generate a configuration file from defaults and dump it to stdout.
    GenerateConfig {},

    /// Run the validator node.
    ///
    /// Loads the configuration values from the given configuration file or uses defaults if not
    /// given, then launches the reactor.
    Validator {
        #[structopt(short, long, env)]
        /// Path to configuration file.
        config: Option<path::PathBuf>,

        /// Override log-level, forcing debug output.
        #[structopt(short, long)]
        debug: bool,
    },
}

impl Cli {
    /// Execute selected CLI command.
    pub async fn run(self) -> anyhow::Result<()> {
        match self {
            Cli::GenerateCert { output } => {
                if output.file_name().is_none() {
                    anyhow::bail!("not a valid output path");
                }

                let mut cert_path = output.clone();
                cert_path.set_extension("crt.pem");

                let mut key_path = output.clone();
                key_path.set_extension("key.pem");

                let (cert, key) = tls::generate_node_cert()?;

                tls::save_cert(&cert, cert_path)?;
                tls::save_private_key(&key, key_path)?;

                Ok(())
            }
            Cli::GenerateConfig {} => {
                let cfg_str = config::to_string(&Default::default())?;
                io::stdout().write_all(cfg_str.as_bytes())?;

                Ok(())
            }
            Cli::Validator { config, debug } => {
                // We load the specified config, if any, otherwise use defaults.
                let mut cfg = config
                    .map(config::load_from_file)
                    .transpose()?
                    .unwrap_or_default();
                if debug {
                    cfg.log.level = tracing::Level::DEBUG;
                }
                cfg.log.setup_logging()?;

                reactor::launch::<reactor::validator::Reactor>(cfg).await
            }
        }
    }
}
