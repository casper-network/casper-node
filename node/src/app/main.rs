//! # CasperLabs blockchain node
//!
//! This is the core application for the CasperLabs blockchain. Run with `--help` to see available
//! command-line arguments.

mod cli;
pub mod config;

use std::{
    panic::{set_hook, PanicInfo},
    process::abort,
};

use backtrace::Backtrace;
use structopt::StructOpt;

use cli::Cli;

/// Aborting panic hook.
///
/// Will exit the application using `abort` when an error occurs. Always shows a backtrace.
fn panic_hook(info: &PanicInfo) {
    let backtrace = Backtrace::new();

    eprintln!("{:?}", backtrace);

    // Print panic info
    if let Some(s) = info.payload().downcast_ref::<&str>() {
        eprintln!("node panicked: {}", s);
    // TODO - use `info.message()` once https://github.com/rust-lang/rust/issues/66745 is fixed
    // } else if let Some(message) = info.message() {
    //     eprintln!("{}", message);
    } else {
        eprintln!("{}", info);
    }

    // Abort after a panic, even if only a worker thread panicked.
    abort()
}

#[tokio::main]
pub async fn main() -> anyhow::Result<()> {
    set_hook(Box::new(panic_hook));

    // Parse CLI args and run selected subcommand.
    let opts = Cli::from_args();
    opts.run().await
}
