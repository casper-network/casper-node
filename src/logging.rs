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

/// Initializes the logging system.
///
/// This function should only be called once during the lifetime of the application.
pub fn init() -> anyhow::Result<()> {
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

    Ok(())
}
