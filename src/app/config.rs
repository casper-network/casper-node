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

use ansi_term::Style;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use tracing::{debug, Event, Level, Subscriber};
use tracing_subscriber::{
    fmt::{
        format,
        time::{FormatTime, SystemTime},
        FmtContext, FormatEvent, FormatFields,
    },
    prelude::*,
    registry::LookupSpan,
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
#[derive(Debug, Deserialize, Serialize)]
pub struct Log {
    /// Log level.
    #[serde(with = "log_level")]
    pub level: Level,
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

impl Default for Log {
    fn default() -> Self {
        Log { level: Level::INFO }
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
        let meta = event.metadata();

        let style = Style::new().dimmed();
        write!(writer, "{}", style.prefix())?;
        SystemTime.format_time(writer)?;
        write!(writer, "{}", style.suffix())?;

        let color = log_level::color(meta.level());
        write!(
            writer,
            " {}{:<6}{}",
            color.prefix(),
            meta.level().to_string(),
            color.suffix()
        )?;

        // TODO - enable outputting spans.  See
        // https://github.com/tokio-rs/tracing/blob/21f28f74/tracing-subscriber/src/fmt/format/mod.rs#L667-L695
        // for details.
        //
        // let full_ctx = FullCtx::new(&ctx);
        // write!(writer, "{}", full_ctx)?;

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
            style.prefix(),
            module,
            file,
            line,
            style.suffix()
        )?;

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
                .with_max_level(self.level.clone())
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

/// Serialization/deserialization
mod log_level {
    use std::str::FromStr;

    use ansi_term::Color;
    use serde::{self, de::Error, Deserialize, Deserializer, Serializer};
    use tracing::Level;

    pub fn serialize<S>(value: &Level, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(value.to_string().as_str())
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Level, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;

        Level::from_str(s.as_str()).map_err(Error::custom)
    }

    pub(super) fn color(value: &Level) -> Color {
        match *value {
            Level::TRACE => Color::Purple,
            Level::DEBUG => Color::Blue,
            Level::INFO => Color::Green,
            Level::WARN => Color::Yellow,
            Level::ERROR => Color::Red,
        }
    }
}
