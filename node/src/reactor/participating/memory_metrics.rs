use datasize::DataSize;
use prometheus::{self, Histogram, HistogramOpts, IntGauge, Registry};
use tracing::debug;

use super::Reactor;
use crate::unregister_metric;

/// Metrics for estimated heap memory usage for the participating reactor.
#[derive(Debug)]
pub(super) struct MemoryMetrics {
    mem_total: IntGauge,
    mem_metrics: IntGauge,
    mem_net: IntGauge,
    mem_address_gossiper: IntGauge,
    mem_storage: IntGauge,
    mem_contract_runtime: IntGauge,
    mem_rpc_server: IntGauge,
    mem_rest_server: IntGauge,
    mem_event_stream_server: IntGauge,
    mem_chainspec_loader: IntGauge,
    mem_consensus: IntGauge,
    mem_deploy_fetcher: IntGauge,
    mem_deploy_gossiper: IntGauge,
    mem_block_proposer: IntGauge,
    mem_block_validator: IntGauge,
    mem_linear_chain: IntGauge,
    /// Histogram detailing how long it took to measure memory usage.
    mem_estimator_runtime_s: Histogram,
    registry: Registry,
}

impl MemoryMetrics {
    /// Initializes a new set of memory metrics.
    pub(super) fn new(registry: Registry) -> Result<Self, prometheus::Error> {
        let mem_total = IntGauge::new("mem_total", "total memory usage in bytes")?;
        let mem_metrics = IntGauge::new("mem_metrics", "metrics memory usage in bytes")?;
        let mem_net = IntGauge::new("mem_net", "network memory usage in bytes")?;
        let mem_address_gossiper = IntGauge::new(
            "mem_address_gossiper",
            "address_gossiper memory usage in bytes",
        )?;
        let mem_storage = IntGauge::new("mem_storage", "storage memory usage in bytes")?;
        let mem_contract_runtime = IntGauge::new(
            "mem_contract_runtime",
            "contract runtime memory usage in bytes",
        )?;
        let mem_rpc_server = IntGauge::new("mem_rpc_server", "rpc server memory usage in bytes")?;
        let mem_rest_server =
            IntGauge::new("mem_rest_server", "rest server memory usage in bytes")?;
        let mem_event_stream_server = IntGauge::new(
            "mem_event_stream_server",
            "event stream server memory usage in bytes",
        )?;
        let mem_chainspec_loader = IntGauge::new(
            "mem_chainspec_loader",
            "chainspec loader memory usage in bytes",
        )?;
        let mem_consensus = IntGauge::new("mem_consensus", "consensus memory usage in bytes")?;
        let mem_deploy_fetcher =
            IntGauge::new("mem_deploy_fetcher", "deploy fetcher memory usage in bytes")?;
        let mem_deploy_gossiper = IntGauge::new(
            "mem_deploy_gossiper",
            "deploy gossiper memory usage in bytes",
        )?;
        let mem_block_proposer =
            IntGauge::new("mem_block_proposer", "block proposer memory usage in bytes")?;
        let mem_block_validator = IntGauge::new(
            "mem_block_validator",
            "block validator memory usage in bytes",
        )?;
        let mem_linear_chain =
            IntGauge::new("mem_linear_chain", "linear chain memory usage in bytes")?;

        let mem_estimator_runtime_s = Histogram::with_opts(
            HistogramOpts::new(
                "mem_estimator_runtime_s",
                "time in seconds to estimate memory usage",
            )
            //  Create buckets from one nanosecond to eight seconds.
            .buckets(prometheus::exponential_buckets(0.000_000_004, 2.0, 32)?),
        )?;

        registry.register(Box::new(mem_total.clone()))?;
        registry.register(Box::new(mem_metrics.clone()))?;
        registry.register(Box::new(mem_net.clone()))?;
        registry.register(Box::new(mem_address_gossiper.clone()))?;
        registry.register(Box::new(mem_storage.clone()))?;
        registry.register(Box::new(mem_contract_runtime.clone()))?;
        registry.register(Box::new(mem_rpc_server.clone()))?;
        registry.register(Box::new(mem_rest_server.clone()))?;
        registry.register(Box::new(mem_event_stream_server.clone()))?;
        registry.register(Box::new(mem_chainspec_loader.clone()))?;
        registry.register(Box::new(mem_consensus.clone()))?;
        registry.register(Box::new(mem_deploy_fetcher.clone()))?;
        registry.register(Box::new(mem_deploy_gossiper.clone()))?;
        registry.register(Box::new(mem_block_proposer.clone()))?;
        registry.register(Box::new(mem_block_validator.clone()))?;
        registry.register(Box::new(mem_linear_chain.clone()))?;
        registry.register(Box::new(mem_estimator_runtime_s.clone()))?;

        Ok(MemoryMetrics {
            mem_total,
            mem_metrics,
            mem_net,
            mem_address_gossiper,
            mem_storage,
            mem_contract_runtime,
            mem_rpc_server,
            mem_rest_server,
            mem_event_stream_server,
            mem_chainspec_loader,
            mem_consensus,
            mem_deploy_fetcher,
            mem_deploy_gossiper,
            mem_block_proposer,
            mem_block_validator,
            mem_linear_chain,
            mem_estimator_runtime_s,
            registry,
        })
    }

    /// Estimates memory usage and updates metrics.
    pub(super) fn estimate(&self, reactor: &Reactor) {
        let timer = self.mem_estimator_runtime_s.start_timer();

        let metrics = reactor.metrics.estimate_heap_size() as i64;
        let net = reactor.small_network.estimate_heap_size() as i64;
        let address_gossiper = reactor.address_gossiper.estimate_heap_size() as i64;
        let storage = reactor.storage.estimate_heap_size() as i64;
        let contract_runtime = reactor.contract_runtime.estimate_heap_size() as i64;
        let rpc_server = reactor.rpc_server.estimate_heap_size() as i64;
        let rest_server = reactor.rest_server.estimate_heap_size() as i64;
        let event_stream_server = reactor.event_stream_server.estimate_heap_size() as i64;
        let chainspec_loader = reactor.chainspec_loader.estimate_heap_size() as i64;
        let consensus = reactor.consensus.estimate_heap_size() as i64;
        let deploy_fetcher = reactor.deploy_fetcher.estimate_heap_size() as i64;
        let deploy_gossiper = reactor.deploy_gossiper.estimate_heap_size() as i64;
        let block_proposer = reactor.block_proposer.estimate_heap_size() as i64;
        let block_validator = reactor.block_validator.estimate_heap_size() as i64;
        let linear_chain = reactor.linear_chain.estimate_heap_size() as i64;

        let total = metrics
            + net
            + address_gossiper
            + storage
            + contract_runtime
            + rpc_server
            + rest_server
            + event_stream_server
            + chainspec_loader
            + consensus
            + deploy_fetcher
            + deploy_gossiper
            + block_proposer
            + block_validator
            + linear_chain;

        self.mem_total.set(total);
        self.mem_metrics.set(metrics);
        self.mem_net.set(net);
        self.mem_address_gossiper.set(address_gossiper);
        self.mem_storage.set(storage);
        self.mem_contract_runtime.set(contract_runtime);
        self.mem_rpc_server.set(rpc_server);
        self.mem_rest_server.set(rest_server);
        self.mem_event_stream_server.set(event_stream_server);
        self.mem_chainspec_loader.set(chainspec_loader);
        self.mem_consensus.set(consensus);
        self.mem_deploy_fetcher.set(deploy_fetcher);
        self.mem_deploy_gossiper.set(deploy_gossiper);
        self.mem_block_proposer.set(block_proposer);
        self.mem_block_validator.set(block_validator);
        self.mem_linear_chain.set(linear_chain);

        // Stop the timer explicitly, don't count logging.
        let duration_s = timer.stop_and_record();

        debug!(%total,
               %duration_s,
               %metrics,
               %net,
               %address_gossiper,
               %storage,
               %contract_runtime,
               %rpc_server,
               %rest_server,
               %event_stream_server,
               %chainspec_loader,
               %consensus,
               %deploy_fetcher,
               %deploy_gossiper,
               %block_proposer,
               %block_validator,
               %linear_chain,
               "Collected new set of memory metrics.");
    }
}

impl Drop for MemoryMetrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.mem_total);
        unregister_metric!(self.registry, self.mem_metrics);
        unregister_metric!(self.registry, self.mem_net);
        unregister_metric!(self.registry, self.mem_address_gossiper);
        unregister_metric!(self.registry, self.mem_storage);
        unregister_metric!(self.registry, self.mem_contract_runtime);
        unregister_metric!(self.registry, self.mem_rpc_server);
        unregister_metric!(self.registry, self.mem_rest_server);
        unregister_metric!(self.registry, self.mem_event_stream_server);
        unregister_metric!(self.registry, self.mem_chainspec_loader);
        unregister_metric!(self.registry, self.mem_consensus);
        unregister_metric!(self.registry, self.mem_deploy_fetcher);
        unregister_metric!(self.registry, self.mem_deploy_gossiper);
        unregister_metric!(self.registry, self.mem_block_proposer);
        unregister_metric!(self.registry, self.mem_block_validator);
        unregister_metric!(self.registry, self.mem_linear_chain);
        unregister_metric!(self.registry, self.mem_estimator_runtime_s);
    }
}
