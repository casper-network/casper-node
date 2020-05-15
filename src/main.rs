//! # CasperLabs blockchain node
//!
//! This crate contain the core application for the CasperLabs blockchain. Run with `--help` to
//! see available command-line arguments.
//!
//! ## Application structure
//!
//! While the `main` function is the central entrypoint for the node application, its core event
//! loop is found inside the reactor. To get a tour of the sourcecode, be sure to run
//! `cargo doc --open`.

mod cli;
mod config;
mod effect;
mod util;

use structopt::StructOpt;

/// Parse command-line arguments and run application.
#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    // Parse CLI args and run selected subcommand.
    let opts = cli::Cli::from_args();
    opts.run().await
}
