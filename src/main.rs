//! # CasperLabs blockchain node
//!
//! This crate contain the core application for the CasperLabs blockchain. Run with `--help` to see
//! available command-line arguments.
//!
//! ## Application structure
//!
//! While the [`main`](fn.main.html) function is the central entrypoint for the node application,
//! its core event loop is found inside the [reactor](reactor/index.html). To get a tour of the
//! sourcecode, be sure to run `cargo doc --open`.

#![doc(
    html_favicon_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Favicon_RGB_50px.png",
    html_logo_url = "https://raw.githubusercontent.com/CasperLabs/casper-node/master/images/CasperLabs_Logo_Symbol_RGB.png",
    test(attr(forbid(warnings)))
)]
#![warn(
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    // unreachable_pub,
    unused_qualifications
)]

mod cli;
mod components;
mod config;
mod effect;
mod reactor;
mod tls;
mod utils;

use structopt::StructOpt;

/// Parses [command-line arguments](cli/index.html) and run application.
#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    // Parse CLI args and run selected subcommand.
    let opts = cli::Cli::from_args();
    opts.run().await
}
