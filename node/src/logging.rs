//! Logging via the tracing crate.

use std::{fmt, io};

use ansi_term::{Color, Style};
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

/// This is used to implement tracing's `FormatEvent` so that we can customize the way tracing
/// events are formatted.
struct FmtEvent {
    // Whether module segments should be shortened to first letter only.
    single_letter: bool,
}

impl FmtEvent {
    fn new(single_letter_module: bool) -> Self {
        FmtEvent {
            single_letter: single_letter_module,
        }
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
            if self.single_letter {
                full_module_path
                    .split("::")
                    .map(|segment| &segment[..1])
                    .collect::<Vec<_>>()
                    .join("::")
            } else {
                full_module_path.to_owned()
            }
        };

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

/// Initializes the logging system with the full module path in the log entries.
pub fn init() -> anyhow::Result<()> {
    init_params(false)
}

/// Initializes the logging system.
///
/// This function should only be called once during the lifetime of the application. Do not call
/// this outside of the application or testing code, the installed logger is global.
///
/// If `single_letter_module` is set to `true` then short module path will be used:
/// instead of `foo::bar::baz::bizz` it will output `f::b::b::b`.
/// Only module path is affected, file name will still be printed in full.
///
/// See the `README.md` for hints on how to configure logging at runtime.
pub fn init_params(single_letter_module: bool) -> anyhow::Result<()> {
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
            .with_writer(io::stdout)
            .with_env_filter(EnvFilter::from_default_env())
            .fmt_fields(formatter)
            .event_format(FmtEvent::new(single_letter_module))
            .finish(),
    )?;

    Ok(())
}
