use prometheus::{IntCounter, Registry};

use crate::unregister_metric;

const BINARY_PORT_TRY_ACCEPT_TRANSACTION_COUNT_NAME: &str =
    "binary_port_try_accept_transaction_count";
const BINARY_PORT_TRY_ACCEPT_TRANSACTION_COUNT_HELP: &str =
    "number of TryAcceptTransaction queries received";

const BINARY_PORT_TRY_SPECULATIVE_EXEC_COUNT_NAME: &str = "binary_port_try_speculative_exec_count";
const BINARY_PORT_TRY_SPECULATIVE_EXEC_COUNT_HELP: &str =
    "number of TrySpeculativeExec queries received";

const BINARY_PORT_GET_DB_COUNT_NAME: &str = "binary_port_get_db_count";
const BINARY_PORT_GET_DB_COUNT_HELP: &str = "number of received Get queries related to DB";

const BINARY_PORT_GET_NON_PERSISTED_DATA_COUNT_NAME: &str =
    "binary_port_get_non_persisted_data_count";
const BINARY_PORT_GET_NON_PERSISTED_DATA_COUNT_HELP: &str =
    "number of received Get queries related to non persisted data";

const BINARY_PORT_GET_STATE_COUNT_NAME: &str = "binary_port_get_state_count";
const BINARY_PORT_GET_STATE_COUNT_HELP: &str = "number of Get State queries received";

const BINARY_PORT_GET_ALL_VALUES_COUNT_NAME: &str = "binary_port_get_all_values_count";
const BINARY_PORT_GET_ALL_VALUES_COUNT_HELP: &str = "number of Get All Values queries received";

const BINARY_PORT_GET_TRIE_COUNT_NAME: &str = "binary_port_get_trie_count";
const BINARY_PORT_GET_TRIE_COUNT_HELP: &str = "number of Get Trie queries received";

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
    /// Number of `Get::Db` queries received.
    pub(super) binary_port_get_db_count: IntCounter,
    /// Number of `Get::NonPersistedData` queries received.
    pub(super) binary_port_get_non_persisted_data_count: IntCounter,
    /// Number of `Get::State` queries received.
    pub(super) binary_port_get_state_count: IntCounter,
    /// Number of `Get::AllValues` queries received.
    pub(super) binary_port_get_all_values_count: IntCounter,
    /// Number of `Get::Trie` queries received.
    pub(super) binary_port_get_trie_count: IntCounter,
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

        let binary_port_get_db_count = IntCounter::new(
            BINARY_PORT_GET_DB_COUNT_NAME.to_string(),
            BINARY_PORT_GET_DB_COUNT_HELP.to_string(),
        )?;

        let binary_port_get_non_persisted_data_count = IntCounter::new(
            BINARY_PORT_GET_NON_PERSISTED_DATA_COUNT_NAME.to_string(),
            BINARY_PORT_GET_NON_PERSISTED_DATA_COUNT_HELP.to_string(),
        )?;

        let binary_port_get_state_count = IntCounter::new(
            BINARY_PORT_GET_STATE_COUNT_NAME.to_string(),
            BINARY_PORT_GET_STATE_COUNT_HELP.to_string(),
        )?;

        let binary_port_get_all_values_count = IntCounter::new(
            BINARY_PORT_GET_ALL_VALUES_COUNT_NAME.to_string(),
            BINARY_PORT_GET_ALL_VALUES_COUNT_HELP.to_string(),
        )?;

        let binary_port_get_trie_count = IntCounter::new(
            BINARY_PORT_GET_TRIE_COUNT_NAME.to_string(),
            BINARY_PORT_GET_TRIE_COUNT_HELP.to_string(),
        )?;

        let binary_port_connections_count = IntCounter::new(
            BINARY_PORT_CONNECTIONS_COUNT_NAME.to_string(),
            BINARY_PORT_CONNECTIONS_COUNT_HELP.to_string(),
        )?;

        registry.register(Box::new(binary_port_try_accept_transaction_count.clone()))?;
        registry.register(Box::new(binary_port_try_speculative_exec_count.clone()))?;
        registry.register(Box::new(binary_port_get_db_count.clone()))?;
        registry.register(Box::new(binary_port_get_non_persisted_data_count.clone()))?;
        registry.register(Box::new(binary_port_get_state_count.clone()))?;
        registry.register(Box::new(binary_port_get_all_values_count.clone()))?;
        registry.register(Box::new(binary_port_get_trie_count.clone()))?;
        registry.register(Box::new(binary_port_connections_count.clone()))?;

        Ok(Metrics {
            binary_port_try_accept_transaction_count,
            binary_port_try_speculative_exec_count,
            binary_port_get_db_count,
            binary_port_get_non_persisted_data_count,
            binary_port_get_state_count,
            binary_port_get_all_values_count,
            binary_port_get_trie_count,
            binary_port_connections_count,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.binary_port_try_accept_transaction_count);
        unregister_metric!(self.registry, self.binary_port_try_speculative_exec_count);
        unregister_metric!(self.registry, self.binary_port_get_db_count);
        unregister_metric!(self.registry, self.binary_port_get_non_persisted_data_count);
        unregister_metric!(self.registry, self.binary_port_get_state_count);
        unregister_metric!(self.registry, self.binary_port_get_all_values_count);
        unregister_metric!(self.registry, self.binary_port_get_trie_count);
        unregister_metric!(self.registry, self.binary_port_connections_count);
    }
}
