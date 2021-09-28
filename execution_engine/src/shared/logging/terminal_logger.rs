use std::sync::atomic::{AtomicUsize, Ordering};

use log::{Level, LevelFilter, Log, Metadata, Record};

use crate::shared::logging::{
    structured_message::{MessageId, MessageProperties, StructuredMessage, TimestampRfc3999},
    Settings, Style, CASPER_METADATA_TARGET, DEFAULT_MESSAGE_KEY, METRIC_METADATA_TARGET,
};

#[doc(hidden)]
/// Logs messages from targets with prefix "casper_" or "METRIC" to stdout.
pub struct TerminalLogger {
    max_level: LevelFilter,
    metrics_enabled: bool,
    style: Style,
    next_message_id: AtomicUsize,
}

impl TerminalLogger {
    /// Creates new terminal logger instance.
    pub fn new(settings: &Settings) -> Self {
        TerminalLogger {
            max_level: settings.max_level(),
            metrics_enabled: settings.enable_metrics(),
            style: settings.style(),
            next_message_id: AtomicUsize::new(0),
        }
    }

    /// Preapres new log line.
    pub fn prepare_log_line(&self, record: &Record) -> Option<String> {
        if !self.enabled(record.metadata()) {
            return None;
        }

        let mut properties = MessageProperties::default();
        let _ = record.key_values().visit(&mut properties);

        let log_line = match self.style {
            Style::Structured => {
                if record.key_values().count() == 0 {
                    properties.insert(
                        DEFAULT_MESSAGE_KEY.to_string(),
                        format!("{}", record.args()),
                    );
                }

                let message_id =
                    MessageId::new(self.next_message_id.fetch_add(1, Ordering::SeqCst));
                let structured_message = StructuredMessage::new(
                    level_to_str(record).to_string(),
                    message_id,
                    properties,
                );
                format!("{}", structured_message)
            }
            Style::HumanReadable => {
                let formatted_properties = properties.get_formatted_message();
                let msg = format!("{}", record.args());
                format!(
                    "{timestamp} {level} [{file}:{line}] {msg}{space}{formatted_properties}",
                    timestamp = TimestampRfc3999::default(),
                    level = level_to_str(record).to_uppercase(),
                    file = record.file().unwrap_or("unknown-file"),
                    line = record.line().unwrap_or_default(),
                    msg = msg,
                    space = if formatted_properties.is_empty() || msg.is_empty() {
                        ""
                    } else {
                        " "
                    },
                    formatted_properties = formatted_properties
                )
            }
        };

        Some(log_line)
    }
}

impl Log for TerminalLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        // If the target starts "casper_" it's either come from a log macro in one of our
        // crates, or via `logging::log_details`.  In this case, check the level.
        (metadata.target().starts_with(CASPER_METADATA_TARGET)
            && metadata.level() <= self.max_level)
            // Otherwise, check if the target is "METRIC" and if we have metric logging enabled.
            || (self.metrics_enabled && metadata.target() == METRIC_METADATA_TARGET)
    }

    fn log(&self, record: &Record) {
        if let Some(log_line) = self.prepare_log_line(record) {
            println!("{}", log_line);
        }
    }

    fn flush(&self) {}
}

fn level_to_str<'a>(record: &'a Record) -> &'a str {
    if record.target() == METRIC_METADATA_TARGET {
        return "Metric";
    }

    match record.level() {
        Level::Trace => "Trace",
        Level::Debug => "Debug",
        Level::Info => "Info",
        Level::Warn => "Warn",
        Level::Error => "Error",
    }
}
