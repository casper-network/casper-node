use prometheus::{IntCounter, Registry};

/// Metrics for the gossiper component.
#[derive(Debug)]
pub struct GossiperMetrics {
    /// Total number of items received by the gossiper.
    items_received: IntCounter,

    /// Reference to the registry for unregistering.
    registry: Registry,
}

impl GossiperMetrics {
    /// Creates a new instance of gossiper metrics, using the given prefix.
    pub fn new(name: &str, registry: &Registry) -> Result<Self, prometheus::Error> {
        let items_received = IntCounter::new(
            format!("{}_items_received", name),
            format!(
                "number of items received by the {} gossiper component",
                name
            ),
        )?;

        registry.register(Box::new(items_received.clone()))?;

        Ok(GossiperMetrics {
            items_received,
            registry: registry.clone(),
        })
    }
}

impl Drop for GossiperMetrics {
    fn drop(&mut self) {
        self.registry
            .unregister(Box::new(self.items_received.clone()))
            .expect("did not expect deregistering items_received to fail");
    }
}
