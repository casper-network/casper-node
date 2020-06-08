//! Configuration file management.
//!
//! Configuration for the node is loaded from TOML files, but all configuration values have sensible
//! defaults.
//!
//! The binary offers an option to generate a configuration from defaults for editing. I.e. running
//! the following will dump a default configuration file to stdout:
//! ```
//! cargo run --release -- generate-config
//! ```
//!
//! # Adding a configuration section
//!
//! When adding a section to the configuration, ensure that
//!
//! * it has an entry in the root configuration [`Config`](struct.Config.html),
//! * `Default` is implemented (derived or manually) with sensible defaults, and
//! * it is completely documented.

use std::{fmt, fs, io, path::Path};

use ansi_term::{Color, Style};
use anyhow::Context;
use serde::{Deserialize, Serialize};
use tracing::{debug, Event, Level, Subscriber};
use tracing_subscriber::{
    fmt::{
        format,
        time::{FormatTime, SystemTime},
        FmtContext, FormatEvent, FormatFields, FormattedFields,
    },
    prelude::*,
    registry::LookupSpan,
    EnvFilter,
};

use casperlabs_node::Config as SmallNetworkConfig;

/// Root configuration.
#[derive(Debug, Deserialize, Serialize)]
pub struct Config {
    /// Log configuration.
    pub log: Log,
    /// Network configuration for the validator-only network.
    pub validator_net: SmallNetworkConfig,
    /// Network configuration for the public network.
    pub public_net: SmallNetworkConfig,
}

/// Log configuration.
#[derive(Debug, Default, Deserialize, Serialize)]
pub struct Log {
    // TODO: There currently is no configuration for logging. Either there need to be requirements
//       for how logging should be configurable or implemented, or this configuration section
//       should be removed.
}

impl Default for Config {
    fn default() -> Self {
        Config {
            log: Default::default(),
            validator_net: SmallNetworkConfig::default_on_port(34553),
            public_net: SmallNetworkConfig::default_on_port(1485),
        }
    }
}

/// This is used to implement tracing's `FormatEvent` so that we can customize the way tracing
/// events are formatted.
struct FmtEvent {}

impl<S, N> FormatEvent<S, N> for FmtEvent
where
    S: Subscriber + for<'a> LookupSpan<'a>,
    N: for<'a> FormatFields<'a> + 'static,
{
    fn format_event(
        &self,
        ctx: &FmtContext<'_, S, N>,
        writer: &mut dyn fmt::Write,
        event: &Event<'_>,
    ) -> fmt::Result {
        // print the date/time with dimmed style
        let dimmed = Style::new().dimmed();
        write!(writer, "{}", dimmed.prefix())?;
        SystemTime.format_time(writer)?;
        write!(writer, "{}", dimmed.suffix())?;

        // print the log level in color
        let meta = event.metadata();
        let color = match *meta.level() {
            Level::TRACE => Color::Purple,
            Level::DEBUG => Color::Blue,
            Level::INFO => Color::Green,
            Level::WARN => Color::Yellow,
            Level::ERROR => Color::Red,
        };

        write!(
            writer,
            " {}{:<6}{}",
            color.prefix(),
            meta.level().to_string(),
            color.suffix()
        )?;

        // print the span information as per
        // https://github.com/tokio-rs/tracing/blob/21f28f74/tracing-subscriber/src/fmt/format/mod.rs#L667-L695
        let mut span_seen = false;

        ctx.visit_spans(|span| {
            write!(writer, "{}", span.metadata().name())?;
            span_seen = true;

            let ext = span.extensions();
            let fields = &ext
                .get::<FormattedFields<N>>()
                .expect("Unable to find FormattedFields in extensions; this is a bug");
            if !fields.is_empty() {
                write!(writer, "{{{}}}", fields)?;
            }
            writer.write_char(':')
        })?;

        if span_seen {
            writer.write_char(' ')?;
        }

        // print the module path, filename and line number with dimmed style
        let module = meta.module_path().unwrap_or_default();

        let file = meta
            .file()
            .unwrap_or_default()
            .rsplitn(2, '/')
            .next()
            .unwrap_or_default();

        let line = meta.line().unwrap_or_default();

        write!(
            writer,
            "{}[{} {}:{}]{} ",
            dimmed.prefix(),
            module,
            file,
            line,
            dimmed.suffix()
        )?;

        // print the log message and other fields
        ctx.format_fields(writer, event)?;
        writeln!(writer)
    }
}

impl Log {
    /// Initializes logging system based on settings in configuration.
    ///
    /// Will setup logging as described in this configuration for the whole application. This
    /// function should only be called once during the lifetime of the application.
    pub fn setup_logging(&self) -> anyhow::Result<()> {
        let formatter = format::debug_fn(|writer, field, value| {
            if field.name() == "message" {
                write!(writer, "{:?}", value)
            } else {
                write!(writer, "{}={:?}", field, value)
            }
        })
        .delimited("; ");

        // Setup a new tracing-subscriber writing to `stderr` for logging.
        tracing::subscriber::set_global_default(
            tracing_subscriber::fmt()
                .with_writer(io::stderr)
                .with_env_filter(EnvFilter::from_default_env())
                .fmt_fields(formatter)
                .event_format(FmtEvent {})
                .finish(),
        )?;
        debug!("debug output enabled");

        Ok(())
    }
}

/// Loads a TOML-formatted configuration from a given file.
pub fn load_from_file<P: AsRef<Path>>(config_path: P) -> anyhow::Result<Config> {
    let path_ref = config_path.as_ref();
    Ok(toml::from_str(
        &fs::read_to_string(path_ref)
            .with_context(|| format!("Failed to read configuration file {:?}", path_ref))?,
    )
    .with_context(|| format!("Failed to parse configuration file {:?}", path_ref))?)
}

/// Creates a TOML-formatted string from a given configuration.
pub fn to_string(cfg: &Config) -> anyhow::Result<String> {
    toml::to_string_pretty(cfg).with_context(|| "Failed to serialize default configuration")
}
