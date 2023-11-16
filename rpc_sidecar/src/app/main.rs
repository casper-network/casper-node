use std::{
    env, fmt, fs, io,
    net::{SocketAddr, ToSocketAddrs},
    path::{Path, PathBuf},
    sync::Arc,
};

use anyhow::Context;
use casper_rpc_sidecar::{run_server, Config, JulietNodeClient};
use casper_types::ProtocolVersion;

use hyper::{
    server::{conn::AddrIncoming, Builder as ServerBuilder},
    Server,
};
use tokio::runtime::Builder;

use structopt::StructOpt;
use tracing::warn;
use tracing_subscriber::fmt::format;

use std::{
    panic::{self, PanicInfo},
    process,
};

use backtrace::Backtrace;

use tracing::field::Field;
use tracing_subscriber::{fmt::format::Writer, EnvFilter};

/// The maximum thread count which should be spawned by the tokio runtime.
pub const MAX_THREAD_COUNT: usize = 512;

/// Main function.
fn main() -> anyhow::Result<()> {
    let num_cpus = num_cpus::get();
    let runtime = Builder::new_multi_thread()
        .enable_all()
        .worker_threads(num_cpus)
        .max_blocking_threads(MAX_THREAD_COUNT - num_cpus)
        .build()
        .unwrap();

    panic::set_hook(Box::new(panic_hook));

    // Parse CLI args and run selected subcommand.
    let opts = Cli::from_args();
    let config = load_config(&opts.config)?;
    init_logging()?;
    runtime.block_on(run(config, opts.protocol_version))
}

async fn run(config: Config, version: ProtocolVersion) -> anyhow::Result<()> {
    let builder = start_listening(&config.address)?;
    let (node_client, client_loop) = JulietNodeClient::new(([127, 0, 0, 1], 28104)).await;
    let server_loop = run_server(
        Arc::new(node_client),
        builder,
        version,
        config.qps_limit,
        config.max_body_bytes,
        config.cors_origin.clone(),
    );
    tokio::join!(client_loop, server_loop);
    Ok(())
}

fn start_listening(address: &str) -> anyhow::Result<ServerBuilder<AddrIncoming>> {
    let address = resolve_address(address).map_err(|error| {
        warn!(%error, %address, "failed to start HTTP server, cannot parse address");
        error
    })?;

    Server::try_bind(&address).map_err(|error| {
        warn!(%error, %address, "failed to start HTTP server");
        error.into()
    })
}

/// Parses a network address from a string, with DNS resolution.
fn resolve_address(address: &str) -> anyhow::Result<SocketAddr> {
    address
        .to_socket_addrs()?
        .next()
        .ok_or_else(|| anyhow::anyhow!("failed to resolve address"))
}

fn load_config(config: &Path) -> anyhow::Result<Config> {
    // The app supports running without a config file, using default values.
    let encoded_config = fs::read_to_string(config)
        .context("could not read configuration file")
        .with_context(|| config.display().to_string())?;

    // Get the TOML table version of the config indicated from CLI args, or from a new
    // defaulted config instance if one is not provided.
    let mut config_table: toml::value::Table = toml::from_str(&encoded_config)?;
    let config = config_table
        .remove("rpc_server")
        .ok_or_else(|| anyhow::anyhow!("sidecar config not found"))?;

    Ok(config.try_into()?)
}

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

/// Initializes the logging system.
///
/// This function should only be called once during the lifetime of the application. Do not call
/// this outside of the application or testing code, the installed logger is global.
#[allow(trivial_casts)]
fn init_logging() -> anyhow::Result<()> {
    const LOG_CONFIGURATION_ENVVAR: &str = "RUST_LOG";

    const LOG_FIELD_MESSAGE: &str = "message";
    const LOG_FIELD_TARGET: &str = "log.target";
    const LOG_FIELD_MODULE: &str = "log.module_path";
    const LOG_FIELD_FILE: &str = "log.file";
    const LOG_FIELD_LINE: &str = "log.line";

    type FormatDebugFn = fn(&mut Writer, &Field, &dyn std::fmt::Debug) -> fmt::Result;

    fn format_into_debug_writer(
        writer: &mut Writer,
        field: &Field,
        value: &dyn fmt::Debug,
    ) -> fmt::Result {
        match field.name() {
            LOG_FIELD_MESSAGE => write!(writer, "{:?}", value),
            LOG_FIELD_TARGET | LOG_FIELD_MODULE | LOG_FIELD_FILE | LOG_FIELD_LINE => Ok(()),
            _ => write!(writer, "; {}={:?}", field, value),
        }
    }

    let formatter = format::debug_fn(format_into_debug_writer as FormatDebugFn);

    let filter = EnvFilter::new(
        env::var(LOG_CONFIGURATION_ENVVAR)
            .as_deref()
            .unwrap_or("warn,casper_rpc_sidecar=info"),
    );

    let builder = tracing_subscriber::fmt()
        .with_writer(io::stdout as fn() -> io::Stdout)
        .with_env_filter(filter)
        .fmt_fields(formatter)
        .with_filter_reloading();
    builder.try_init().map_err(|error| anyhow::anyhow!(error))?;
    Ok(())
}

// Note: The docstring on `Cli` is the help shown when calling the binary with `--help`.
#[derive(Debug, StructOpt)]
#[allow(rustdoc::invalid_html_tags)]
/// Casper blockchain node.
pub struct Cli {
    /// Path to configuration file.
    config: PathBuf,
    /// Version of the protocol.
    protocol_version: ProtocolVersion,
}

#[cfg(test)]
mod tests {
    use std::fs;

    use regex::Regex;
    use schemars::schema_for_value;

    use crate::{rpcs::docs::OPEN_RPC_SCHEMA, testing::assert_schema};

    #[test]
    fn json_schema_check() {
        let schema_path = format!(
            "{}/../resources/test/rpc_schema.json",
            env!("CARGO_MANIFEST_DIR")
        );
        assert_schema(&schema_path, schema_for_value!(OPEN_RPC_SCHEMA.clone()));
        let schema = fs::read_to_string(&schema_path).unwrap();

        // Check for the following pattern in the JSON as this points to a byte array or vec (e.g.
        // a hash digest) not being represented as a hex-encoded string:
        //
        // ```json
        // "type": "array",
        // "items": {
        //   "type": "integer",
        //   "format": "uint8",
        //   "minimum": 0.0
        // },
        // ```
        let regex = Regex::new(
            r#"\s*"type":\s*"array",\s*"items":\s*\{\s*"type":\s*"integer",\s*"format":\s*"uint8",\s*"minimum":\s*0\.0\s*\},"#
        ).unwrap();
        assert!(
            !regex.is_match(&schema),
            "seems like a byte array is not hex-encoded"
        );
    }
}
