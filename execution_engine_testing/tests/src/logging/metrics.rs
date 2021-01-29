use std::sync::{Arc, Mutex};

use log::{Metadata, Record};

use casper_execution_engine::shared::logging::TerminalLogger;

struct Logger {
    terminal_logger: TerminalLogger,
    log_lines: Arc<Mutex<Vec<String>>>,
}

impl log::Log for Logger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        self.terminal_logger.enabled(metadata)
    }

    fn log(&self, record: &Record) {
        if let Some(log_line) = self.terminal_logger.prepare_log_line(record) {
            self.log_lines.lock().unwrap().push(log_line);
        }
    }

    fn flush(&self) {}
}
