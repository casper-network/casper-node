use prometheus::{IntCounter, IntCounterVec, Opts, Registry};

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

const BINARY_PORT_TRIE_COUNT_NAME: &str = "binary_port_get_trie_count";
const BINARY_PORT_TRIE_COUNT_HELP: &str = "number of Get queries received for the trie state";

const BINARY_PORT_DROPPED_CONNECTIONS_COUNT_NAME: &str = "binary_port_dropped_connections_count";
const BINARY_PORT_DROPPED_CONNECTIONS_COUNT_HELP: &str =
    "number of binary port connections dropped by the node, by reason";
const BINARY_PORT_DROPPED_CONNECTIONS_REASON_LABEL: &str = "reason";

/// Reason for which the node dropped a binary port connection.
///
/// Connections closed by the peer, or closed after serving the single request of a short-lived
/// connection, are not drops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DropReason {
    /// The connection limit was reached when the connection was accepted.
    ConnectionLimitReached,
    /// The connection's lifetime ran out while waiting for or receiving a request.
    LifetimeExpired,
    /// The connection's lifetime ran out while a response was being sent.
    LifetimeExpiredWhileSendingResponse,
    /// The peer sent a request too slowly.
    ReceiveTooSlow,
    /// The peer read a response too slowly.
    SendTooSlow,
    /// The peer sent a request exceeding the maximum message size.
    RequestTooLarge,
    /// The peer sent a request without a payload.
    EmptyRequest,
    /// The peer sent a request which couldn't be decoded.
    InvalidRequest,
    /// A response couldn't be serialized.
    ResponseSerializationFailed,
    /// An I/O error occurred on the connection.
    Io,
}

impl DropReason {
    const ALL: [DropReason; 10] = [
        DropReason::ConnectionLimitReached,
        DropReason::LifetimeExpired,
        DropReason::LifetimeExpiredWhileSendingResponse,
        DropReason::ReceiveTooSlow,
        DropReason::SendTooSlow,
        DropReason::RequestTooLarge,
        DropReason::EmptyRequest,
        DropReason::InvalidRequest,
        DropReason::ResponseSerializationFailed,
        DropReason::Io,
    ];

    /// Value of the `reason` label.
    fn label(self) -> &'static str {
        match self {
            DropReason::ConnectionLimitReached => "connection_limit_reached",
            DropReason::LifetimeExpired => "lifetime_expired",
            DropReason::LifetimeExpiredWhileSendingResponse => {
                "lifetime_expired_while_sending_response"
            }
            DropReason::ReceiveTooSlow => "receive_too_slow",
            DropReason::SendTooSlow => "send_too_slow",
            DropReason::RequestTooLarge => "request_too_large",
            DropReason::EmptyRequest => "empty_request",
            DropReason::InvalidRequest => "invalid_request",
            DropReason::ResponseSerializationFailed => "response_serialization_failed",
            DropReason::Io => "io_error",
        }
    }
}

/// Metrics.
#[derive(Debug)]
pub(crate) struct Metrics {
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
    /// Number of `Get::Trie` queries received.
    pub(super) binary_port_get_trie_count: IntCounter,
    /// Number of connections dropped by the node, labeled by [`DropReason`].
    binary_port_dropped_connections_count: IntCounterVec,

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

        let binary_port_get_trie_count = IntCounter::new(
            BINARY_PORT_TRIE_COUNT_NAME.to_string(),
            BINARY_PORT_TRIE_COUNT_HELP.to_string(),
        )?;

        let binary_port_dropped_connections_count = IntCounterVec::new(
            Opts::new(
                BINARY_PORT_DROPPED_CONNECTIONS_COUNT_NAME,
                BINARY_PORT_DROPPED_CONNECTIONS_COUNT_HELP,
            ),
            &[BINARY_PORT_DROPPED_CONNECTIONS_REASON_LABEL],
        )?;
        // Export every reason, including those which never occurred, as 0.
        for reason in DropReason::ALL {
            binary_port_dropped_connections_count.with_label_values(&[reason.label()]);
        }

        registry.register(Box::new(binary_port_try_accept_transaction_count.clone()))?;
        registry.register(Box::new(binary_port_try_speculative_exec_count.clone()))?;
        registry.register(Box::new(binary_port_get_record_count.clone()))?;
        registry.register(Box::new(binary_port_get_info_count.clone()))?;
        registry.register(Box::new(binary_port_get_state_count.clone()))?;
        registry.register(Box::new(binary_port_connections_count.clone()))?;
        registry.register(Box::new(binary_port_get_trie_count.clone()))?;
        registry.register(Box::new(binary_port_dropped_connections_count.clone()))?;

        Ok(Metrics {
            binary_port_try_accept_transaction_count,
            binary_port_try_speculative_exec_count,
            binary_port_get_record_count,
            binary_port_get_info_count,
            binary_port_get_state_count,
            binary_port_connections_count,
            binary_port_get_trie_count,
            binary_port_dropped_connections_count,
            registry: registry.clone(),
        })
    }
}

impl Metrics {
    /// Records that the node dropped a connection for the given reason.
    pub(super) fn record_dropped_connection(&self, reason: DropReason) {
        self.binary_port_dropped_connections_count
            .with_label_values(&[reason.label()])
            .inc();
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
        unregister_metric!(self.registry, self.binary_port_get_trie_count);
        unregister_metric!(self.registry, self.binary_port_dropped_connections_count);
    }
}
