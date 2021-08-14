//! Logging via the tracing crate.

use std::{env, fmt, io};

use ansi_term::{Color, Style};
use anyhow::anyhow;
use datasize::DataSize;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use tracing::{
    field::{Field, Visit},
    Event, Level, Subscriber,
};
use tracing_subscriber::{
    fmt::{
        format,
        time::{FormatTime, SystemTime},
        FmtContext, FormatEvent, FormatFields, FormattedFields,
    },
    registry::LookupSpan,
    EnvFilter,
};

const LOG_CONFIGURATION_ENVVAR: &str = "RUST_LOG";

const LOG_FIELD_MESSAGE: &str = "message";
const LOG_FIELD_TARGET: &str = "log.target";
const LOG_FIELD_MODULE: &str = "log.module_path";
const LOG_FIELD_FILE: &str = "log.file";
const LOG_FIELD_LINE: &str = "log.line";

/// Logging configuration.
#[derive(DataSize, Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {
    /// Output format for log.
    format: LoggingFormat,

    /// Colored output (has no effect if JSON format is enabled).
    ///
    /// If set, the logger will inject ANSI color codes into log messages.  This is useful if
    /// writing out to stdout or stderr on an ANSI terminal, but not so if writing to a logfile.
    color: bool,

    /// Abbreviate module names (has no effect if JSON format is enabled).
    ///
    /// If set, human-readable formats will abbreviate module names, `foo::bar::baz::bizz` will
    /// turn into `f:b:b:bizz`.
    abbreviate_modules: bool,
}

impl LoggingConfig {
    /// Creates a new instance of LoggingConfig.
    #[cfg(test)]
    pub fn new(format: LoggingFormat, color: bool, abbreviate_modules: bool) -> Self {
        LoggingConfig {
            format,
            color,
            abbreviate_modules,
        }
    }
}

/// Logging output format.
///
/// Defaults to "text"".
#[derive(DataSize, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LoggingFormat {
    /// Text format.
    Text,
    /// JSON format.
    Json,
}

impl Default for LoggingFormat {
    fn default() -> Self {
        LoggingFormat::Text
    }
}

/// This is used to implement tracing's `FormatEvent` so that we can customize the way tracing
/// events are formatted.
struct FmtEvent {
    /// Whether to use ANSI color formatting or not.
    ansi_color: bool,
    /// Whether module segments should be shortened to first letter only.
    abbreviate_modules: bool,
}

impl FmtEvent {
    fn new(ansi_color: bool, abbreviate_modules: bool) -> Self {
        FmtEvent {
            ansi_color,
            abbreviate_modules,
        }
    }

    fn enable_dimmed_if_ansi(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
        if self.ansi_color {
            write!(writer, "{}", Style::new().dimmed().prefix())
        } else {
            Ok(())
        }
    }

    fn disable_dimmed_if_ansi(&self, writer: &mut dyn fmt::Write) -> fmt::Result {
        if self.ansi_color {
            write!(writer, "{}", Style::new().dimmed().suffix())
        } else {
            Ok(())
        }
    }
}

// Used to gather the relevant details from the fields applied by the `tracing_log::LogTracer`,
// which is used by logging macros when dependent crates use `log` rather than `tracing`.
#[derive(Default)]
struct FieldVisitor {
    module: Option<String>,
    file: Option<String>,
    line: Option<u32>,
}

impl Visit for FieldVisitor {
    fn record_str(&mut self, field: &Field, value: &str) {
        if field.name() == LOG_FIELD_MODULE {
            self.module = Some(value.to_string())
        } else if field.name() == LOG_FIELD_FILE {
            self.file = Some(value.to_string())
        }
    }

    fn record_u64(&mut self, field: &Field, value: u64) {
        if field.name() == LOG_FIELD_LINE {
            self.line = Some(value as u32)
        }
    }

    fn record_debug(&mut self, _field: &Field, _value: &dyn fmt::Debug) {}
}

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
        // print the date/time with dimmed style if `ansi_color` is true
        self.enable_dimmed_if_ansi(writer)?;
        SystemTime.format_time(writer)?;
        self.disable_dimmed_if_ansi(writer)?;

        // print the log level
        let meta = event.metadata();
        if self.ansi_color {
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
        } else {
            write!(writer, " {:<6}", meta.level().to_string())?;
        }

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

        // print the module path, filename and line number with dimmed style if `ansi_color` is true
        let mut field_visitor = FieldVisitor::default();
        event.record(&mut field_visitor);
        let module = {
            let full_module_path = meta
                .module_path()
                .or_else(|| field_visitor.module.as_deref())
                .unwrap_or_default();
            if self.abbreviate_modules {
                // Use a smallvec for going up to six levels deep.
                let mut parts: SmallVec<[&str; 6]> = full_module_path.split("::").collect();

                let count = parts.len();
                // Abbreviate all but last segment.
                if count > 1 {
                    for part in parts.iter_mut().take(count - 1) {
                        assert!(part.is_ascii());
                        *part = &part[0..1];
                    }
                }
                // Use a single `:` to join the abbreviated modules to make the output even shorter.
                parts.join(":")
            } else {
                full_module_path.to_owned()
            }
        };

        let file = if !self.abbreviate_modules {
            meta.file()
                .or_else(|| field_visitor.file.as_deref())
                .unwrap_or_default()
                .rsplitn(2, '/')
                .next()
                .unwrap_or_default()
        } else {
            ""
        };

        let line = meta.line().or(field_visitor.line).unwrap_or_default();

        if !module.is_empty() && (!file.is_empty() || self.abbreviate_modules) {
            self.enable_dimmed_if_ansi(writer)?;
            write!(writer, "[{} {}:{}] ", module, file, line,)?;
            self.disable_dimmed_if_ansi(writer)?;
        }

        // print the log message and other fields
        ctx.format_fields(writer, event)?;
        writeln!(writer)
    }
}

/// Initializes the logging system with the default parameters.
///
/// See `init_params` for details.
#[cfg(test)]
pub fn init() -> anyhow::Result<()> {
    init_with_config(&Default::default())
}

/// Initializes the logging system.
///
/// This function should only be called once during the lifetime of the application. Do not call
/// this outside of the application or testing code, the installed logger is global.
///
/// See the `README.md` for hints on how to configure logging at runtime.
pub fn init_with_config(config: &LoggingConfig) -> anyhow::Result<()> {
    let formatter = format::debug_fn(|writer, field, value| match field.name() {
        LOG_FIELD_MESSAGE => write!(writer, "{:?}", value),
        LOG_FIELD_TARGET | LOG_FIELD_MODULE | LOG_FIELD_FILE | LOG_FIELD_LINE => Ok(()),
        _ => write!(writer, "; {}={:?}", field, value),
    });

    let filter = EnvFilter::new(
        env::var(LOG_CONFIGURATION_ENVVAR)
            .as_deref()
            .unwrap_or("warn,casper_node=info"),
    );

    match config.format {
        // Setup a new tracing-subscriber writing to `stdout` for logging.
        LoggingFormat::Text => tracing_subscriber::fmt()
            .with_writer(io::stdout)
            .with_env_filter(filter)
            .fmt_fields(formatter)
            .event_format(FmtEvent::new(config.color, config.abbreviate_modules))
            .try_init(),
        // JSON logging writes to `stdout` as well but uses the JSON format.
        LoggingFormat::Json => tracing_subscriber::fmt()
            .with_writer(io::stdout)
            .with_env_filter(filter)
            .json()
            .try_init(),
    }
    .map_err(|error| anyhow!(error))
}
