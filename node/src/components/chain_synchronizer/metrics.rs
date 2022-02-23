use datasize::DataSize;
use prometheus::{self, IntGauge, Registry};

use crate::unregister_metric;

/// Metrics for the block proposer.
#[derive(DataSize, Debug, Clone)]
#[allow(dead_code)]
pub(super) struct Metrics {
    /// Total time of syncing the chain in seconds.
    #[data_size(skip)]
    pub(super) chain_sync_total_time_seconds: IntGauge,
    /// The time taken by the entire fast sync task.
    #[data_size(skip)]
    pub(super) chain_fast_sync_time_seconds: IntGauge,
    /// The time needed to download the global state.
    #[data_size(skip)]
    pub(super) chain_sync_global_state_download_time_seconds: IntGauge,
    /// The time taken by the archival sync task.
    #[data_size(skip)]
    pub(super) chain_archival_sync_time_seconds: IntGauge,
    /// The time needed to fetch and execute blocks.
    #[data_size(skip)]
    pub(super) chain_sync_fetch_and_execute_blocks_time_seconds: IntGauge,
    /// The time taken by the upgrade process.
    #[data_size(skip)]
    pub(super) chain_sync_upgrade_time_seconds: IntGauge,
    /// The time taken by the emergency restart process.
    #[data_size(skip)]
    pub(super) chain_sync_emergency_restart_time_seconds: IntGauge,
    /// Registry stored to allow deregistration later.
    #[data_size(skip)]
    registry: Registry,
}

impl Metrics {
    /// Creates a new instance of the block proposer metrics.
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let chain_sync_total_time_seconds = IntGauge::new(
            "chain_sync_total_time_seconds",
            "total time of syncing the chain in seconds",
        )?;
        let chain_sync_global_state_download_time_seconds = IntGauge::new(
            "chain_sync_global_state_download_time_seconds",
            "the time needed to download the global state",
        )?;
        let chain_sync_fetch_and_execute_blocks_time_seconds = IntGauge::new(
            "chain_sync_fetch_and_execute_blocks_time_seconds",
            "the time needed to fetch and execute blocks",
        )?;
        let chain_archival_sync_time_seconds = IntGauge::new(
            "chain_archival_sync_time_seconds",
            "the time taken by the entire archival sync task",
        )?;
        let chain_fast_sync_time_seconds = IntGauge::new(
            "chain_fast_sync_time_seconds",
            "the time taken by the entire fast sync sync task",
        )?;
        let chain_sync_upgrade_time_seconds = IntGauge::new(
            "chain_sync_upgrade_time_seconds",
            "the time taken by the upgrade process",
        )?;
        let chain_sync_emergency_restart_time_seconds = IntGauge::new(
            "chain_sync_emergency_restart_time_seconds",
            "the time taken by the emergency restart process",
        )?;

        registry.register(Box::new(chain_sync_total_time_seconds.clone()))?;
        registry.register(Box::new(
            chain_sync_global_state_download_time_seconds.clone(),
        ))?;
        registry.register(Box::new(
            chain_sync_fetch_and_execute_blocks_time_seconds.clone(),
        ))?;
        registry.register(Box::new(chain_archival_sync_time_seconds.clone()))?;

        Ok(Metrics {
            chain_sync_total_time_seconds,
            chain_sync_global_state_download_time_seconds,
            chain_sync_fetch_and_execute_blocks_time_seconds,
            chain_archival_sync_time_seconds,
            chain_fast_sync_time_seconds,
            chain_sync_upgrade_time_seconds,
            chain_sync_emergency_restart_time_seconds,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.chain_sync_total_time_seconds);
    }
}
