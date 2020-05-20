//! # CasperLabs blockchain node
//!
//! This is the core application for the CasperLabs blockchain. Run with `--help` to see available
//! command-line arguments.

mod cli;
pub mod config;

use structopt::StructOpt;

use cli::Cli;

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    // Parse CLI args and run selected subcommand.
    let opts = Cli::from_args();
    opts.run().await
}
