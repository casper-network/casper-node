use datasize::DataSize;
use prometheus::{self, IntGauge, Registry};

/// Metrics for the block proposer.
#[derive(DataSize, Debug, Clone)]
#[allow(dead_code)]
pub(super) struct Metrics {
    /// Total time in seconds of syncing the chain.
    #[data_size(skip)]
    pub(super) chain_sync_total_duration_seconds: IntGauge,
    /// Time in seconds of handling the emergency restart.
    #[data_size(skip)]
    pub(super) chain_sync_emergency_restart_duration_seconds: IntGauge,
    /// Time in seconds of handling the upgrade.
    #[data_size(skip)]
    pub(super) chain_sync_upgrade_duration_seconds: IntGauge,
    /// Total time in seconds of performing the sync to genesis..
    #[data_size(skip)]
    pub(super) chain_sync_to_genesis_total_duration_seconds: IntGauge,
    /// Time in seconds to get the trusted key block.
    #[data_size(skip)]
    pub(super) chain_sync_get_trusted_key_block_info_duration_seconds: IntGauge,
    /// Time in seconds to fetch to genesis during sync to genesis.
    #[data_size(skip)]
    pub(super) chain_sync_fetch_to_genesis_duration_seconds: IntGauge,
    /// Time in seconds to fetch forward during sync to genesis.
    #[data_size(skip)]
    pub(super) chain_sync_fetch_forward_duration_seconds: IntGauge,
    /// Total time in seconds of performing the fast sync.
    #[data_size(skip)]
    pub(super) chain_sync_fast_sync_total_duration_seconds: IntGauge,
    /// Time in seconds of fetching block headers during fast sync.
    #[data_size(skip)]
    pub(super) chain_sync_fetch_block_headers_duration_seconds: IntGauge,
    /// Time in seconds of fetching block headers for replay protection during fast sync.
    #[data_size(skip)]
    pub(super) chain_sync_replay_protection_duration_seconds: IntGauge,
    /// Time in seconds of fetching block headers for era supervisor initialization during fast
    /// sync.
    #[data_size(skip)]
    pub(super) chain_sync_era_supervisor_init_duration_seconds: IntGauge,
    /// Time in seconds of executing blocks during chain sync.
    #[data_size(skip)]
    pub(super) chain_sync_execute_blocks_duration_seconds: IntGauge,
    /// Time in seconds of fetching the initial trusted block header during chain sync.
    #[data_size(skip)]
    pub(super) chain_sync_fetch_and_store_initial_trusted_block_header_duration_seconds: IntGauge,
    /// Time in seconds of syncing trie store (global state download) during chain sync.
    #[data_size(skip)]
    pub(super) chain_sync_sync_trie_store_duration_seconds: IntGauge,
    /// Registry stored to allow deregistration later.
    #[data_size(skip)]
    registry: Registry,
}

impl Metrics {
    /// Creates a new instance of the block proposer metrics.
    pub fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let chain_sync_total_duration_seconds = IntGauge::new(
            "chain_sync_total_duration_seconds",
            "total time in seconds of syncing the chain",
        )?;
        let chain_sync_emergency_restart_duration_seconds = IntGauge::new(
            "chain_sync_emergency_restart_duration_seconds",
            "time in seconds of handling the emergency restart",
        )?;
        let chain_sync_upgrade_duration_seconds = IntGauge::new(
            "chain_sync_upgrade_duration_seconds",
            "time in seconds of handling the upgrade",
        )?;
        let chain_sync_to_genesis_total_duration_seconds = IntGauge::new(
            "chain_sync_to_genesis_total_duration_seconds",
            "total time in seconds of performing the sync to genesis",
        )?;
        let chain_sync_get_trusted_key_block_info_duration_seconds = IntGauge::new(
            "chain_sync_get_trusted_key_block_info_duration_seconds",
            "time in seconds to get the trusted key block",
        )?;
        let chain_sync_fetch_to_genesis_duration_seconds = IntGauge::new(
            "chain_sync_fetch_to_genesis_duration_seconds",
            "time in seconds to fetch to genesis during sync to genesis",
        )?;
        let chain_sync_fetch_forward_duration_seconds = IntGauge::new(
            "chain_sync_fetch_forward_duration_seconds",
            "time in seconds to fetch forward during sync to genesis",
        )?;
        let chain_sync_fast_sync_total_duration_seconds = IntGauge::new(
            "chain_sync_fast_sync_total_duration_seconds",
            "total time in seconds of performing the fast sync",
        )?;
        let chain_sync_fetch_block_headers_duration_seconds = IntGauge::new(
            "chain_sync_fetch_block_headers_duration_seconds",
            "time in seconds of fetching block headers during fast sync",
        )?;
        let chain_sync_replay_protection_duration_seconds = IntGauge::new(
            "chain_sync_replay_protection_duration_seconds",
            "time in seconds of fetching block headers for replay protection during fast sync",
        )?;
        let chain_sync_era_supervisor_init_duration_seconds = IntGauge::new(
            "chain_sync_era_supervisor_init_duration_seconds",
            "time in seconds of fetching block headers for era supervisor initialization during fast sync",
        )?;
        let chain_sync_execute_blocks_duration_seconds = IntGauge::new(
            "chain_sync_execute_blocks_duration_seconds",
            "time in seconds of executing blocks during chain sync",
        )?;
        let chain_sync_fetch_and_store_initial_trusted_block_header_duration_seconds =
            IntGauge::new(
                "chain_sync_fetch_and_store_initial_trusted_block_header_duration_seconds",
                "time in seconds of fetching the initial trusted block header during chain sync",
            )?;
        let chain_sync_sync_trie_store_duration_seconds = IntGauge::new(
            "chain_sync_sync_trie_store_duration_seconds",
            "time in seconds of syncing trie store (global state download) during chain sync",
        )?;

        registry.register(Box::new(chain_sync_total_duration_seconds.clone()))?;
        registry.register(Box::new(
            chain_sync_emergency_restart_duration_seconds.clone(),
        ))?;
        registry.register(Box::new(chain_sync_upgrade_duration_seconds.clone()))?;
        registry.register(Box::new(
            chain_sync_to_genesis_total_duration_seconds.clone(),
        ))?;
        registry.register(Box::new(
            chain_sync_get_trusted_key_block_info_duration_seconds.clone(),
        ))?;
        registry.register(Box::new(
            chain_sync_fetch_to_genesis_duration_seconds.clone(),
        ))?;
        registry.register(Box::new(chain_sync_fetch_forward_duration_seconds.clone()))?;
        registry.register(Box::new(
            chain_sync_fast_sync_total_duration_seconds.clone(),
        ))?;
        registry.register(Box::new(
            chain_sync_fetch_block_headers_duration_seconds.clone(),
        ))?;
        registry.register(Box::new(
            chain_sync_replay_protection_duration_seconds.clone(),
        ))?;
        registry.register(Box::new(
            chain_sync_era_supervisor_init_duration_seconds.clone(),
        ))?;
        registry.register(Box::new(chain_sync_execute_blocks_duration_seconds.clone()))?;
        registry.register(Box::new(
            chain_sync_sync_trie_store_duration_seconds.clone(),
        ))?;

        Ok(Metrics {
            chain_sync_total_duration_seconds,
            chain_sync_emergency_restart_duration_seconds,
            chain_sync_upgrade_duration_seconds,
            chain_sync_to_genesis_total_duration_seconds,
            chain_sync_get_trusted_key_block_info_duration_seconds,
            chain_sync_fetch_to_genesis_duration_seconds,
            chain_sync_fetch_forward_duration_seconds,
            chain_sync_fast_sync_total_duration_seconds,
            chain_sync_fetch_block_headers_duration_seconds,
            chain_sync_replay_protection_duration_seconds,
            chain_sync_era_supervisor_init_duration_seconds,
            chain_sync_execute_blocks_duration_seconds,
            chain_sync_fetch_and_store_initial_trusted_block_header_duration_seconds,
            chain_sync_sync_trie_store_duration_seconds,
            registry: registry.clone(),
        })
    }
}

impl Drop for Metrics {
    fn drop(&mut self) {
        // All metrics should be unregistered here.
        // They are, however, attached to the `joiner` reactor which means they are going to be lost
        // as soon as the node transforms to `participating`.

        // As a workaround we keep these metrics registered, until all reactors are unified.

        // unregister_metric!(self.registry, self.chain_sync_total_duration_seconds);
        // unregister_metric!(
        //     self.registry,
        //     self.chain_sync_fetch_and_store_block_header_duration_seconds
        // );
        // unregister_metric!(...
    }
}
