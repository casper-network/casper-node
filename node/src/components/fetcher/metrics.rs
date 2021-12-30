use prometheus::{IntCounter, Registry};

use crate::unregister_metric;

#[derive(Debug)]
pub(crate) struct Metrics {
    /// Number of fetch requests that found an item in the storage.
    pub found_in_storage: IntCounter,
    /// Number of fetch requests that fetched an item from peer.
    pub found_on_peer: IntCounter,
    /// Number of fetch requests that timed out.
    pub timeouts: IntCounter,
    /// Reference to the registry for unregistering.
    registry: Registry,
}

impl Metrics {
    pub(super) fn new(name: &str, registry: &Registry) -> Result<Self, prometheus::Error> {
        let found_in_storage = IntCounter::new(
            format!("{}_found_in_storage", name),
            format!(
                "number of fetch requests that found {} in local storage",
                name
            ),
        )?;
        let found_on_peer = IntCounter::new(
            format!("{}_found_on_peer", name),
            format!("number of fetch requests that fetched {} from peer", name),
        )?;
        let timeouts = IntCounter::new(
            format!("{}_timeouts", name),
            format!("number of {} fetch requests that timed out", name),
        )?;
        registry.register(Box::new(found_in_storage.clone()))?;
        registry.register(Box::new(found_on_peer.clone()))?;
        registry.register(Box::new(timeouts.clone()))?;

        Ok(Metrics {
            found_in_storage,
            found_on_peer,
            timeouts,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.found_in_storage);
        unregister_metric!(self.registry, self.found_on_peer);
        unregister_metric!(self.registry, self.timeouts);
    }
}
