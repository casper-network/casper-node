use prometheus::{IntCounter, Registry};

use crate::unregister_metric;

const BINARY_PORT_GET_COUNT_NAME: &str = "binary_port_get_count";
const BINARY_PORT_GET_COUNT_HELP: &str = "number of Get queries received";

/// Metrics.
#[derive(Debug)]
pub(super) struct Metrics {
    /// Number of Get queries received.
    pub(super) binary_port_get_count: IntCounter,

    registry: Registry,
}

impl Metrics {
    /// Creates a new instance of the metrics.
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let binary_port_get_count = IntCounter::new(
            "BINARY_PORT_GET_COUNT_NAME".to_string(),
            "BINARY_PORT_GET_COUNT_HELP".to_string(),
        )?;
        registry.register(Box::new(binary_port_get_count.clone()))?;

        Ok(Metrics {
            binary_port_get_count,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.binary_port_get_count);
    }
}
