//! # Casper blockchain node
//!
//! This is the core application for the Casper blockchain. Run with `--help` to see available
//! command-line arguments.

mod cli;
pub mod config;

use std::{
    panic::{self, PanicInfo},
    process,
};

use backtrace::Backtrace;
use structopt::StructOpt;
use tokio::runtime::Builder;
use tracing::info;

use casper_node::MAX_THREAD_COUNT;

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
    process::abort()
}

/// Main function.
fn main() -> anyhow::Result<()> {
    // The exit code is determined in a block to ensure that all acquired resources are dropped
    // before exiting with the given exit code.
    let exit_code = {
        let mut runtime = Builder::new()
            .threaded_scheduler()
            .enable_all()
            .max_threads(MAX_THREAD_COUNT)
            .build()
            .unwrap();

        panic::set_hook(Box::new(panic_hook));

        // Parse CLI args and run selected subcommand.
        let opts = Cli::from_args();

        runtime.block_on(async { opts.run().await })?
    };

    info!(%exit_code, "exiting casper-node");
    process::exit(exit_code)
}
