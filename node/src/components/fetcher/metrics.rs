use prometheus::{IntCounter, Registry};
use tracing::warn;

#[derive(Debug)]
pub struct FetcherMetrics {
    /// Number of fetch requests that found an item in the storage.
    pub(super) found_in_storage: IntCounter,
    /// Number of fetch requests that fetched an item from peer.
    pub(super) found_on_peer: IntCounter,
    /// Number of fetch requests that timed out.
    pub(super) timeouts: IntCounter,
    /// Reference to the registry for unregistering.
    registry: Registry,
}

impl FetcherMetrics {
    pub fn new(name: &str, registry: &Registry) -> Result<Self, prometheus::Error> {
        let found_in_storage = IntCounter::new(
            format!("{}_found_in_storage", name),
            format!(
                "number of fetch requests that found {} in the storage.",
                name
            ),
        )?;
        let found_on_peer = IntCounter::new(
            format!("{}_found_on_peer", name),
            format!("number of fetch requests that fetched {} from peer.", name),
        )?;
        let timeouts = IntCounter::new(
            format!("{}_timeouts", name),
            format!("number of {} fetch requests that timed out", name),
        )?;
        registry.register(Box::new(found_in_storage.clone()))?;
        registry.register(Box::new(found_on_peer.clone()))?;
        registry.register(Box::new(timeouts.clone()))?;

        Ok(FetcherMetrics {
            found_in_storage,
            found_on_peer,
            timeouts,
            registry: registry.clone(),
        })
    }
}

impl Drop for FetcherMetrics {
    fn drop(&mut self) {
        self.registry
            .unregister(Box::new(self.found_in_storage.clone()))
            .unwrap_or_else(
                |err| warn!(%err, "did not expect deregistering found_in_storage to fail"),
            );
        self.registry
            .unregister(Box::new(self.found_on_peer.clone()))
            .unwrap_or_else(
                |err| warn!(%err, "did not expect deregistering found_on_peer to fail"),
            );
        self.registry
            .unregister(Box::new(self.timeouts.clone()))
            .unwrap_or_else(|err| warn!(%err, "did not expect deregistering timeouts to fail"));
    }
}
