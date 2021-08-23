use datasize::DataSize;
use prometheus::{self, Histogram, HistogramOpts, IntGauge, Registry};
use tracing::debug;

use super::Reactor;
use crate::unregister_metric;

///Metrics for memory usage for the joiner
#[derive(Debug)]
pub(super) struct MemoryMetrics {
    /// Total estimated heap memory usage.
    mem_total: IntGauge,

    /// Estimated heap memory usage of metrics component.
    mem_metrics: IntGauge,
    /// Estimated heap memory usage of network component.
    mem_network: IntGauge,
    /// Estimated heap memory usage of small_network component.
    mem_small_network: IntGauge,
    /// Estimated heap memory usage of address_gossiper component.
    mem_address_gossiper: IntGauge,
    /// Estimated heap memory usage of the configuration for the validator node.
    mem_config: IntGauge,
    /// Estimated heap memory usage for the chainspec loader component.
    mem_chainspec_loader: IntGauge,
    /// Estimated heap memory usage of storage component.
    mem_storage: IntGauge,
    /// Estimated heap memory usage of the contract runtime component.
    mem_contract_runtime: IntGauge,
    /// Estimated heap memory usage of the linear chain fetcher component.
    mem_linear_chain_fetcher: IntGauge,
    /// Estimated heap memory usage of linear chain sync.
    mem_linear_chain_sync: IntGauge,
    /// Estimated heap memory usage of block validator component.
    mem_block_validator: IntGauge,
    /// Estimated heap memory usage of deploy fetcher component.
    mem_deploy_fetcher: IntGauge,
    /// Estimated heap memory usage of linear chain component.
    mem_linear_chain: IntGauge,

    /// Histogram detailing how long it took to estimate memory usage.
    mem_estimator_runtime_s: Histogram,

    /// Instance of registry component to unregister from when being dropped.
    registry: Registry,
}

impl MemoryMetrics {
    /// Initialize a new set of memory metrics for the joiner.
    pub(super) fn new(registry: Registry) -> Result<Self, prometheus::Error> {
        let mem_total = IntGauge::new("joiner_mem_total", "total memory usage in bytes")?;
        let mem_metrics = IntGauge::new("joiner_mem_metrics", "metrics memory usage in bytes")?;
        let mem_network = IntGauge::new("joiner_mem_network", "network memory usage in bytes")?;
        let mem_small_network = IntGauge::new(
            "joiner_mem_small_network",
            "small network memory usage in bytes",
        )?;
        let mem_address_gossiper = IntGauge::new(
            "joiner_mem_address_gossiper",
            "address_gossiper memory usage in bytes",
        )?;
        let mem_config = IntGauge::new("joiner_mem_config", "config memory usage in bytes")?;
        let mem_chainspec_loader = IntGauge::new(
            "joiner_mem_chainspec_loader",
            "chainspec_loader memory usage in bytes",
        )?;
        let mem_storage = IntGauge::new("joiner_mem_storage", "storage memory usage in bytes")?;
        let mem_contract_runtime = IntGauge::new(
            "joiner_mem_contract_runtime",
            "contract_runtime memory usage in bytes",
        )?;
        let mem_linear_chain_fetcher = IntGauge::new(
            "joiner_mem_linear_chain_fetcher",
            "linear_chain_fetcher memory usage in bytes",
        )?;
        let mem_linear_chain_sync = IntGauge::new(
            "joiner_mem_linear_chain_sync",
            "linear_chain_sync memory usage in bytes",
        )?;
        let mem_block_validator = IntGauge::new(
            "joiner_mem_block_validator",
            "block_validator memory usage in bytes",
        )?;
        let mem_deploy_fetcher = IntGauge::new(
            "joiner_mem_deploy_fetcher",
            "deploy_fetcher memory usage in bytes",
        )?;
        let mem_linear_chain = IntGauge::new(
            "joiner_mem_linear_chain",
            "linear_chain memory usage in bytes",
        )?;
        let mem_estimator_runtime_s = Histogram::with_opts(
            HistogramOpts::new(
                "joiner_mem_estimator_runtime_s",
                "time taken to estimate memory usage, in seconds",
            )
            // Create buckets from four nano second to eight seconds
            .buckets(prometheus::exponential_buckets(0.000_000_004, 2.0, 32)?),
        )?;

        registry.register(Box::new(mem_total.clone()))?;
        registry.register(Box::new(mem_metrics.clone()))?;
        registry.register(Box::new(mem_network.clone()))?;
        registry.register(Box::new(mem_small_network.clone()))?;
        registry.register(Box::new(mem_address_gossiper.clone()))?;
        registry.register(Box::new(mem_config.clone()))?;
        registry.register(Box::new(mem_chainspec_loader.clone()))?;
        registry.register(Box::new(mem_storage.clone()))?;
        registry.register(Box::new(mem_contract_runtime.clone()))?;
        registry.register(Box::new(mem_linear_chain_fetcher.clone()))?;
        registry.register(Box::new(mem_linear_chain_sync.clone()))?;
        registry.register(Box::new(mem_block_validator.clone()))?;
        registry.register(Box::new(mem_deploy_fetcher.clone()))?;
        registry.register(Box::new(mem_linear_chain.clone()))?;
        registry.register(Box::new(mem_estimator_runtime_s.clone()))?;

        Ok(MemoryMetrics {
            mem_total,
            mem_metrics,
            mem_network,
            mem_small_network,
            mem_address_gossiper,
            mem_config,
            mem_chainspec_loader,
            mem_storage,
            mem_contract_runtime,
            mem_linear_chain_fetcher,
            mem_linear_chain_sync,
            mem_block_validator,
            mem_deploy_fetcher,
            mem_linear_chain,
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
        let linear_chain_fetcher = reactor.linear_chain_fetcher.estimate_heap_size() as i64;
        let linear_chain_sync = reactor.linear_chain_sync.estimate_heap_size() as i64;
        let block_validator = reactor.block_validator.estimate_heap_size() as i64;
        let deploy_fetcher = reactor.deploy_fetcher.estimate_heap_size() as i64;
        let linear_chain = reactor.linear_chain.estimate_heap_size() as i64;

        let total = metrics
            + small_network
            + address_gossiper
            + config
            + chainspec_loader
            + storage
            + contract_runtime
            + linear_chain_fetcher
            + linear_chain_sync
            + block_validator
            + deploy_fetcher
            + linear_chain;

        self.mem_total.set(total);
        self.mem_metrics.set(metrics);
        self.mem_small_network.set(small_network);
        self.mem_address_gossiper.set(address_gossiper);
        self.mem_config.set(config);
        self.mem_chainspec_loader.set(chainspec_loader);
        self.mem_storage.set(storage);
        self.mem_contract_runtime.set(contract_runtime);
        self.mem_linear_chain_fetcher.set(linear_chain_fetcher);
        self.mem_linear_chain_sync.set(linear_chain_sync);
        self.mem_block_validator.set(block_validator);
        self.mem_deploy_fetcher.set(deploy_fetcher);
        self.mem_linear_chain.set(linear_chain);

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
        %linear_chain_fetcher,
        %linear_chain_sync,
        %block_validator,
        %deploy_fetcher,
        %linear_chain,
        "Collected new set of memory metrics for the joiner");
    }
}

impl Drop for MemoryMetrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.mem_total);
        unregister_metric!(self.registry, self.mem_metrics);
        unregister_metric!(self.registry, self.mem_network);
        unregister_metric!(self.registry, self.mem_small_network);
        unregister_metric!(self.registry, self.mem_address_gossiper);
        unregister_metric!(self.registry, self.mem_config);
        unregister_metric!(self.registry, self.mem_chainspec_loader);
        unregister_metric!(self.registry, self.mem_storage);
        unregister_metric!(self.registry, self.mem_contract_runtime);
        unregister_metric!(self.registry, self.mem_linear_chain_fetcher);
        unregister_metric!(self.registry, self.mem_linear_chain_sync);
        unregister_metric!(self.registry, self.mem_block_validator);
        unregister_metric!(self.registry, self.mem_deploy_fetcher);
        unregister_metric!(self.registry, self.mem_linear_chain);
        unregister_metric!(self.registry, self.mem_estimator_runtime_s);
    }
}
