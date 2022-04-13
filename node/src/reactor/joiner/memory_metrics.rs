use datasize::DataSize;
use prometheus::{self, Histogram, HistogramOpts, IntGauge, Registry};
use tracing::debug;

use super::Reactor;
use crate::unregister_metric;

/// Metrics for estimated heap memory usage for the joiner reactor.
#[derive(Debug)]
pub(super) struct MemoryMetrics {
    mem_total: IntGauge,
    mem_metrics: IntGauge,
    mem_small_network: IntGauge,
    mem_address_gossiper: IntGauge,
    mem_config: IntGauge,
    mem_chainspec_loader: IntGauge,
    mem_storage: IntGauge,
    mem_contract_runtime: IntGauge,
    /// Estimated heap memory usage of the block fetcher component.
    mem_block_fetcher: IntGauge,
    /// Estimated heap memory usage of deploy fetcher component.
    mem_deploy_fetcher: IntGauge,
    /// Histogram detailing how long it took to estimate memory usage.
    mem_estimator_runtime_s: Histogram,
    registry: Registry,
}

impl MemoryMetrics {
    /// Initialize a new set of memory metrics for the joiner.
    pub(super) fn new(registry: Registry) -> Result<Self, prometheus::Error> {
        let mem_total = IntGauge::new("joiner_mem_total", "total memory usage in bytes")?;
        let mem_metrics = IntGauge::new("joiner_mem_metrics", "metrics memory usage in bytes")?;
        let mem_small_network =
            IntGauge::new("joiner_mem_small_network", "network memory usage in bytes")?;
        let mem_address_gossiper = IntGauge::new(
            "joiner_mem_address_gossiper",
            "address gossiper memory usage in bytes",
        )?;
        let mem_config = IntGauge::new("joiner_mem_config", "config memory usage in bytes")?;
        let mem_chainspec_loader = IntGauge::new(
            "joiner_mem_chainspec_loader",
            "chainspec loader memory usage in bytes",
        )?;
        let mem_storage = IntGauge::new("joiner_mem_storage", "storage memory usage in bytes")?;
        let mem_contract_runtime = IntGauge::new(
            "joiner_mem_contract_runtime",
            "contract runtime memory usage in bytes",
        )?;
        let mem_block_fetcher = IntGauge::new(
            "joiner_mem_block_fetcher",
            "block fetcher memory usage in bytes",
        )?;
        let mem_deploy_fetcher = IntGauge::new(
            "joiner_mem_deploy_fetcher",
            "deploy fetcher memory usage in bytes",
        )?;
        let mem_estimator_runtime_s = Histogram::with_opts(
            HistogramOpts::new(
                "joiner_mem_estimator_runtime_s",
                "time in seconds to estimate memory usage",
            )
            // Create buckets from four nano second to eight seconds
            .buckets(prometheus::exponential_buckets(0.000_000_004, 2.0, 32)?),
        )?;

        registry.register(Box::new(mem_total.clone()))?;
        registry.register(Box::new(mem_metrics.clone()))?;
        registry.register(Box::new(mem_small_network.clone()))?;
        registry.register(Box::new(mem_address_gossiper.clone()))?;
        registry.register(Box::new(mem_config.clone()))?;
        registry.register(Box::new(mem_chainspec_loader.clone()))?;
        registry.register(Box::new(mem_storage.clone()))?;
        registry.register(Box::new(mem_contract_runtime.clone()))?;
        registry.register(Box::new(mem_block_fetcher.clone()))?;
        registry.register(Box::new(mem_deploy_fetcher.clone()))?;
        registry.register(Box::new(mem_estimator_runtime_s.clone()))?;

        Ok(MemoryMetrics {
            mem_total,
            mem_metrics,
            mem_small_network,
            mem_address_gossiper,
            mem_config,
            mem_chainspec_loader,
            mem_storage,
            mem_contract_runtime,
            mem_block_fetcher,
            mem_deploy_fetcher,
            mem_estimator_runtime_s,
            registry,
        })
    }

    /// Estimates the memory usage and updates metrics.
    pub(super) fn estimate(&self, reactor: &Reactor) {
        let timer = self.mem_estimator_runtime_s.start_timer();

        let metrics = reactor.metrics.estimate_heap_size() as i64;
        let small_network = reactor.small_network.estimate_heap_size() as i64;
        let address_gossiper = reactor.address_gossiper.estimate_heap_size() as i64;
        let config = reactor.config.estimate_heap_size() as i64;
        let chainspec_loader = reactor.chainspec_loader.estimate_heap_size() as i64;
        let storage = reactor.storage.estimate_heap_size() as i64;
        let contract_runtime = reactor.contract_runtime.estimate_heap_size() as i64;
        let block_fetcher = reactor.block_by_hash_fetcher.estimate_heap_size() as i64;
        let deploy_fetcher = reactor.deploy_fetcher.estimate_heap_size() as i64;

        let total = metrics
            + small_network
            + address_gossiper
            + config
            + chainspec_loader
            + storage
            + contract_runtime
            + block_fetcher
            + deploy_fetcher;

        self.mem_total.set(total);
        self.mem_metrics.set(metrics);
        self.mem_small_network.set(small_network);
        self.mem_address_gossiper.set(address_gossiper);
        self.mem_config.set(config);
        self.mem_chainspec_loader.set(chainspec_loader);
        self.mem_storage.set(storage);
        self.mem_contract_runtime.set(contract_runtime);
        self.mem_block_fetcher.set(block_fetcher);
        self.mem_deploy_fetcher.set(deploy_fetcher);

        // Stop the timer explicitly, don't count logging.
        let duration_s = timer.stop_and_record();

        debug!(
        %total,
        %duration_s,
        %metrics,
        %small_network,
        %address_gossiper,
        %config ,
        %chainspec_loader,
        %storage ,
        %contract_runtime,
        %block_fetcher,
        %deploy_fetcher,
        "Collected new set of memory metrics for the joiner");
    }
}

impl Drop for MemoryMetrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.mem_total);
        unregister_metric!(self.registry, self.mem_metrics);
        unregister_metric!(self.registry, self.mem_small_network);
        unregister_metric!(self.registry, self.mem_address_gossiper);
        unregister_metric!(self.registry, self.mem_config);
        unregister_metric!(self.registry, self.mem_chainspec_loader);
        unregister_metric!(self.registry, self.mem_storage);
        unregister_metric!(self.registry, self.mem_contract_runtime);
        unregister_metric!(self.registry, self.mem_block_fetcher);
        unregister_metric!(self.registry, self.mem_deploy_fetcher);
        unregister_metric!(self.registry, self.mem_estimator_runtime_s);
    }
}
