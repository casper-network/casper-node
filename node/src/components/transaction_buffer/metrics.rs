use prometheus::{IntGauge, Registry};

use crate::unregister_metric;

/// Metrics for the transaction_buffer component.
#[derive(Debug)]
pub(super) struct Metrics {
    /// Total number of transactions contained in the transaction buffer.
    pub(super) total_transactions: IntGauge,
    /// Number of transactions contained in in-flight proposed blocks.
    pub(super) held_transactions: IntGauge,
    /// Number of transactions that should not be included in future proposals ever again.
    pub(super) dead_transactions: IntGauge,
    registry: Registry,
}

impl Metrics {
    /// Creates a new instance of the transaction buffer metrics, using the given prefix.
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let total_transactions = IntGauge::new(
            "transaction_buffer_total_transactions".to_string(),
            "total number of transactions contained in the transaction buffer.".to_string(),
        )?;
        let held_transactions = IntGauge::new(
            "transaction_buffer_held_transactions".to_string(),
            "number of transactions included in in-flight proposed blocks.".to_string(),
        )?;
        let dead_transactions = IntGauge::new(
            "transaction_buffer_dead_transactions".to_string(),
            "number of transactions that should not be included in future proposals.".to_string(),
        )?;

        registry.register(Box::new(total_transactions.clone()))?;
        registry.register(Box::new(held_transactions.clone()))?;
        registry.register(Box::new(dead_transactions.clone()))?;

        Ok(Metrics {
            total_transactions,
            held_transactions,
            dead_transactions,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.total_transactions);
        unregister_metric!(self.registry, self.held_transactions);
        unregister_metric!(self.registry, self.dead_transactions);
    }
}
