use prometheus::{IntCounter, Registry};

use crate::unregister_metric;

const BINARY_PORT_TRY_ACCEPT_TRANSACTION_COUNT_NAME: &str =
    "binary_port_try_accept_transaction_count";
const BINARY_PORT_TRY_ACCEPT_TRANSACTION_COUNT_HELP: &str =
    "number of TryAcceptTransaction queries received";

const BINARY_PORT_TRY_SPECULATIVE_EXEC_COUNT_NAME: &str = "binary_port_try_speculative_exec_count";
const BINARY_PORT_TRY_SPECULATIVE_EXEC_COUNT_HELP: &str =
    "number of TrySpeculativeExec queries received";

const BINARY_PORT_GET_RECORD_COUNT_NAME: &str = "binary_port_get_record_count";
const BINARY_PORT_GET_RECORD_COUNT_HELP: &str = "number of received Get queries for records";

const BINARY_PORT_GET_INFORMATION_NAME: &str = "binary_port_get_info_count";
const BINARY_PORT_GET_INFORMATION_HELP: &str =
    "number of received Get queries for information from the node";

const BINARY_PORT_GET_STATE_COUNT_NAME: &str = "binary_port_get_state_count";
const BINARY_PORT_GET_STATE_COUNT_HELP: &str =
    "number of Get queries received for the global state";

const BINARY_PORT_CONNECTIONS_COUNT_NAME: &str = "binary_port_connections_count";
const BINARY_PORT_CONNECTIONS_COUNT_HELP: &str =
    "total number of external connections established to binary port";

/// Metrics.
#[derive(Debug)]
pub(super) struct Metrics {
    /// Number of `TryAcceptTransaction` queries received.
    pub(super) binary_port_try_accept_transaction_count: IntCounter,
    /// Number of `TrySpeculativeExec` queries received.
    pub(super) binary_port_try_speculative_exec_count: IntCounter,
    /// Number of `Get::Record` queries received.
    pub(super) binary_port_get_record_count: IntCounter,
    /// Number of `Get::Information` queries received.
    pub(super) binary_port_get_info_count: IntCounter,
    /// Number of `Get::State` queries received.
    pub(super) binary_port_get_state_count: IntCounter,
    /// Number of distinct connections to binary port.
    pub(super) binary_port_connections_count: IntCounter,

    registry: Registry,
}

impl Metrics {
    /// Creates a new instance of the metrics.
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let binary_port_try_accept_transaction_count = IntCounter::new(
            BINARY_PORT_TRY_ACCEPT_TRANSACTION_COUNT_NAME.to_string(),
            BINARY_PORT_TRY_ACCEPT_TRANSACTION_COUNT_HELP.to_string(),
        )?;

        let binary_port_try_speculative_exec_count = IntCounter::new(
            BINARY_PORT_TRY_SPECULATIVE_EXEC_COUNT_NAME.to_string(),
            BINARY_PORT_TRY_SPECULATIVE_EXEC_COUNT_HELP.to_string(),
        )?;

        let binary_port_get_record_count = IntCounter::new(
            BINARY_PORT_GET_RECORD_COUNT_NAME.to_string(),
            BINARY_PORT_GET_RECORD_COUNT_HELP.to_string(),
        )?;

        let binary_port_get_info_count = IntCounter::new(
            BINARY_PORT_GET_INFORMATION_NAME.to_string(),
            BINARY_PORT_GET_INFORMATION_HELP.to_string(),
        )?;

        let binary_port_get_state_count = IntCounter::new(
            BINARY_PORT_GET_STATE_COUNT_NAME.to_string(),
            BINARY_PORT_GET_STATE_COUNT_HELP.to_string(),
        )?;

        let binary_port_connections_count = IntCounter::new(
            BINARY_PORT_CONNECTIONS_COUNT_NAME.to_string(),
            BINARY_PORT_CONNECTIONS_COUNT_HELP.to_string(),
        )?;

        registry.register(Box::new(binary_port_try_accept_transaction_count.clone()))?;
        registry.register(Box::new(binary_port_try_speculative_exec_count.clone()))?;
        registry.register(Box::new(binary_port_get_record_count.clone()))?;
        registry.register(Box::new(binary_port_get_info_count.clone()))?;
        registry.register(Box::new(binary_port_get_state_count.clone()))?;
        registry.register(Box::new(binary_port_connections_count.clone()))?;

        Ok(Metrics {
            binary_port_try_accept_transaction_count,
            binary_port_try_speculative_exec_count,
            binary_port_get_record_count,
            binary_port_get_info_count,
            binary_port_get_state_count,
            binary_port_connections_count,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.binary_port_try_accept_transaction_count);
        unregister_metric!(self.registry, self.binary_port_try_speculative_exec_count);
        unregister_metric!(self.registry, self.binary_port_get_record_count);
        unregister_metric!(self.registry, self.binary_port_get_info_count);
        unregister_metric!(self.registry, self.binary_port_get_state_count);
        unregister_metric!(self.registry, self.binary_port_connections_count);
    }
}
