use prometheus::{IntCounter, Registry};

use crate::utils::registered_metric::{RegisteredMetric, RegistryExt};

#[derive(Debug)]
pub(crate) struct Metrics {
    /// Number of fetch requests that found an item in the storage.
    pub found_in_storage: RegisteredMetric<IntCounter>,
    /// Number of fetch requests that fetched an item from peer.
    pub found_on_peer: RegisteredMetric<IntCounter>,
    /// Number of fetch requests that timed out.
    pub timeouts: RegisteredMetric<IntCounter>,
    /// Number of total fetch requests made.
    pub fetch_total: RegisteredMetric<IntCounter>,
}

impl Metrics {
    pub(super) fn new(name: &str, registry: &Registry) -> Result<Self, prometheus::Error> {
        let found_in_storage = registry.new_int_counter(
            format!("{}_found_in_storage", name),
            format!(
                "number of fetch requests that found {} in local storage",
                name
            ),
        )?;
        let found_on_peer = registry.new_int_counter(
            format!("{}_found_on_peer", name),
            format!("number of fetch requests that fetched {} from peer", name),
        )?;
        let timeouts = registry.new_int_counter(
            format!("{}_timeouts", name),
            format!("number of {} fetch requests that timed out", name),
        )?;
        let fetch_total = registry.new_int_counter(
            format!("{}_fetch_total", name),
            format!("number of {} all fetch requests made", name),
        )?;

        Ok(Metrics {
            found_in_storage,
            found_on_peer,
            timeouts,
            fetch_total,
        })
    }
}
