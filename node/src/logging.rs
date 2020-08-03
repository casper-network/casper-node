//! Logging via the tracing crate.

use std::{fmt, io};

use ansi_term::{Color, Style};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use tracing::{Event, Level, Subscriber};
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

/// Logging configuration.
#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LoggingConfig {
    /// Output format for log.
    format: LoggingFormat,
    /// Abbreviate module names.
    ///
    /// If set, human-readable formats will abbreviate module names, `foo::bar::baz::bizz` will turn
    /// into `f:b:b:bizz`.
    abbreviate_modules: bool,
}

/// Logging output format.
///
/// Defaults to "text"".
#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum LoggingFormat {
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
    // Whether module segments should be shortened to first letter only.
    abbreviate_modules: bool,
}

impl FmtEvent {
    fn new(abbreviate_modules: bool) -> Self {
        FmtEvent { abbreviate_modules }
    }
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
        let module = {
            let full_module_path = meta.module_path().unwrap_or_default();
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
                .unwrap_or_default()
                .rsplitn(2, '/')
                .next()
                .unwrap_or_default()
        } else {
            ""
        };

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

/// Initializes the logging system with the default parameters.
///
/// See `init_params` for details.
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
    let formatter = format::debug_fn(|writer, field, value| {
        if field.name() == "message" {
            write!(writer, "{:?}", value)
        } else {
            write!(writer, "{}={:?}", field, value)
        }
    })
    .delimited("; ");

    match config.format {
        // Setup a new tracing-subscriber writing to `stdout` for logging.
        LoggingFormat::Text => tracing::subscriber::set_global_default(
            tracing_subscriber::fmt()
                .with_writer(io::stdout)
                .with_env_filter(EnvFilter::from_default_env())
                .fmt_fields(formatter)
                .event_format(FmtEvent::new(config.abbreviate_modules))
                .finish(),
        )?,
        // JSON logging writes to `stdout` as well but uses the JSON format.
        LoggingFormat::Json => tracing::subscriber::set_global_default(
            tracing_subscriber::fmt()
                .with_writer(io::stdout)
                .with_env_filter(EnvFilter::from_default_env())
                .json()
                .finish(),
        )?,
    }

    Ok(())
}
