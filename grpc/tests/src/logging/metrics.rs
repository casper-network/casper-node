use std::sync::{Arc, Mutex};

use log::{LevelFilter, Metadata, Record};
use serde_json::Value;

use engine_test_support::internal::{InMemoryWasmTestBuilder, MOCKED_ACCOUNT_ADDRESS};
use node::contract_core::engine_state::EngineConfig;
use node::contract_shared::{
    logging::{self, Settings, TerminalLogger, PAYLOAD_KEY},
    newtypes::CorrelationId,
    test_utils,
};
use node::contract_storage::global_state::in_memory::InMemoryGlobalState;

const PROPERTIES_KEY: &str = "properties";
const CORRELATION_ID_KEY: &str = "correlation_id";

struct Logger {
    terminal_logger: TerminalLogger,
    log_lines: Arc<Mutex<Vec<String>>>,
}

impl Logger {
    fn new(buffer: Arc<Mutex<Vec<String>>>, settings: &Settings) -> Self {
        Logger {
            terminal_logger: TerminalLogger::new(settings),
            log_lines: buffer,
        }
    }
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

fn extract_correlation_id_property(line: &str) -> Option<String> {
    if let Some(idx) = line.find(PAYLOAD_KEY) {
        let start = idx + PAYLOAD_KEY.len();
        let end = line.len();
        let slice = &line[start..end];
        serde_json::from_str::<Value>(slice)
            .ok()
            .and_then(|full_value| full_value.get(PROPERTIES_KEY).cloned())
            .and_then(|properties_value| properties_value.get(CORRELATION_ID_KEY).cloned())
            .and_then(|correlation_id_value| correlation_id_value.as_str().map(String::from))
    } else {
        None
    }
}

#[test]
fn should_commit_with_metrics() {
    let settings = Settings::new(LevelFilter::Trace).with_metrics_enabled(true);
    let log_lines = Arc::new(Mutex::new(vec![]));
    let logger = Box::new(Logger::new(Arc::clone(&log_lines), &settings));
    let _ = logging::initialize_with_logger(logger, settings);

    let correlation_id = CorrelationId::new();
    let mocked_account = test_utils::mocked_account(MOCKED_ACCOUNT_ADDRESS);
    let (global_state, root_hash) =
        InMemoryGlobalState::from_pairs(correlation_id, &mocked_account).unwrap();

    let engine_config = EngineConfig::new();

    let result =
        InMemoryWasmTestBuilder::new(global_state, engine_config, root_hash.to_vec()).finish();

    let _commit_response = result
        .builder()
        .commit_transforms(root_hash.to_vec(), Default::default());

    let mut log_lines = log_lines.lock().unwrap();

    let expected_fragment = format!(r#""{}":"{}""#, CORRELATION_ID_KEY, correlation_id);
    log_lines.retain(|line| line.contains(&expected_fragment));
    assert!(
        !log_lines.is_empty(),
        "at least one log line should contain the expected correlation ID"
    );

    for line in log_lines.iter() {
        let extracted_correlation_id = extract_correlation_id_property(line).unwrap();
        assert_eq!(correlation_id.to_string(), extracted_correlation_id);
    }
}
