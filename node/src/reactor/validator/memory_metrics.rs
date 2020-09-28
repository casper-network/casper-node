use datasize::DataSize;
use prometheus::{exponential_buckets, Histogram, HistogramOpts, IntGauge, Registry};
use rand::{CryptoRng, Rng};

use super::Reactor;
use tracing::debug;

/// Metrics for memory usage.
#[derive(Debug)]
pub(super) struct MemoryMetrics {
    /// Total estimated heap memory usage.
    mem_total: IntGauge,

    /// Estimated heap memory usage of metrics component.
    mem_metrics: IntGauge,
    /// Estimated heap memory usage of network component.
    mem_net: IntGauge,
    /// Estimated heap memory usage of address gossiper component.
    mem_address_gossiper: IntGauge,
    /// Estimated heap memory usage of storage component.
    mem_storage: IntGauge,
    /// Estimated heap memory usage of contract runtime component.
    mem_contract_runtime: IntGauge,
    /// Estimated heap memory usage of api server component.
    mem_api_server: IntGauge,
    /// Estimated heap memory usage of chainspec loader component.
    mem_chainspec_loader: IntGauge,
    /// Estimated heap memory usage of consensus component.
    mem_consensus: IntGauge,
    /// Estimated heap memory usage of deploy acceptor component.
    mem_deploy_acceptor: IntGauge,
    /// Estimated heap memory usage of deploy fetcher component.
    mem_deploy_fetcher: IntGauge,
    /// Estimated heap memory usage of deploy gossiper component.
    mem_deploy_gossiper: IntGauge,
    /// Estimated heap memory usage of deploy buffer component.
    mem_deploy_buffer: IntGauge,
    /// Estimated heap memory usage of block executor component.
    mem_block_executor: IntGauge,
    /// Estimated heap memory usage of block validator component.
    mem_proto_block_validator: IntGauge,
    /// Estimated heap memory usage of linear chain component.
    mem_linear_chain: IntGauge,

    /// Histogram detailing how long it took to measure memory usage.
    mem_estimator_runtime_s: Histogram,

    /// Instance of registry to unregister from when being dropped.
    registry: Registry,
}

impl MemoryMetrics {
    /// Initializes a new set of memory metrics.
    pub(super) fn new(registry: Registry) -> Result<Self, prometheus::Error> {
        let mem_total = IntGauge::new("mem_total", "total memory usage in bytes")?;
        let mem_metrics = IntGauge::new("mem_metrics", "metrics memory usage in bytes")?;
        let mem_net = IntGauge::new("mem_net", "net memory usage in bytes")?;
        let mem_address_gossiper = IntGauge::new(
            "mem_address_gossiper",
            "address_gossiper memory usage in bytes",
        )?;
        let mem_storage = IntGauge::new("mem_storage", "storage memory usage in bytes")?;
        let mem_contract_runtime = IntGauge::new(
            "mem_contract_runtime",
            "contract_runtime memory usage in bytes",
        )?;
        let mem_api_server = IntGauge::new("mem_api_server", "api_server memory usage in bytes")?;
        let mem_chainspec_loader = IntGauge::new(
            "mem_chainspec_loader",
            "chainspec_loader memory usage in bytes",
        )?;
        let mem_consensus = IntGauge::new("mem_consensus", "consensus memory usage in bytes")?;
        let mem_deploy_acceptor = IntGauge::new(
            "mem_deploy_acceptor",
            "deploy_acceptor memory usage in bytes",
        )?;
        let mem_deploy_fetcher =
            IntGauge::new("mem_deploy_fetcher", "deploy_fetcher memory usage in bytes")?;
        let mem_deploy_gossiper = IntGauge::new(
            "mem_deploy_gossiper",
            "deploy_gossiper memory usage in bytes",
        )?;
        let mem_deploy_buffer =
            IntGauge::new("mem_deploy_buffer", "deploy_buffer memory usage in bytes")?;
        let mem_block_executor =
            IntGauge::new("mem_block_executor", "block_executor memory usage in bytes")?;
        let mem_proto_block_validator = IntGauge::new(
            "mem_proto_block_validator",
            "proto_block_validator memory usage in bytes",
        )?;
        let mem_linear_chain =
            IntGauge::new("mem_linear_chain", "linear_chain memory usage in bytes")?;

        let mem_estimator_runtime_s = Histogram::with_opts(
            HistogramOpts::new(
                "mem_estimator_runtime_s",
                "time taken to estimate memory usage, in seconds",
            )
            //  Create buckets from one nanosecond to four seconds.
            .buckets(exponential_buckets(0.000_000_001, 2.0, 32)?),
        )?;

        registry.register(Box::new(mem_total.clone()))?;
        registry.register(Box::new(mem_metrics.clone()))?;
        registry.register(Box::new(mem_net.clone()))?;
        registry.register(Box::new(mem_address_gossiper.clone()))?;
        registry.register(Box::new(mem_storage.clone()))?;
        registry.register(Box::new(mem_contract_runtime.clone()))?;
        registry.register(Box::new(mem_api_server.clone()))?;
        registry.register(Box::new(mem_chainspec_loader.clone()))?;
        registry.register(Box::new(mem_consensus.clone()))?;
        registry.register(Box::new(mem_deploy_acceptor.clone()))?;
        registry.register(Box::new(mem_deploy_fetcher.clone()))?;
        registry.register(Box::new(mem_deploy_gossiper.clone()))?;
        registry.register(Box::new(mem_deploy_buffer.clone()))?;
        registry.register(Box::new(mem_block_executor.clone()))?;
        registry.register(Box::new(mem_proto_block_validator.clone()))?;
        registry.register(Box::new(mem_linear_chain.clone()))?;
        registry.register(Box::new(mem_estimator_runtime_s.clone()))?;

        Ok(MemoryMetrics {
            mem_total,
            mem_metrics,
            mem_net,
            mem_address_gossiper,
            mem_storage,
            mem_contract_runtime,
            mem_api_server,
            mem_chainspec_loader,
            mem_consensus,
            mem_deploy_acceptor,
            mem_deploy_fetcher,
            mem_deploy_gossiper,
            mem_deploy_buffer,
            mem_block_executor,
            mem_proto_block_validator,
            mem_linear_chain,
            mem_estimator_runtime_s,
            registry,
        })
    }

    /// Estimates memory usage and updates metrics.
    pub(super) fn estimate<R>(&self, reactor: &Reactor<R>)
    where
        R: CryptoRng + Rng + ?Sized,
    {
        let timer = self.mem_estimator_runtime_s.start_timer();

        let metrics = reactor.metrics.estimate_heap_size() as i64;
        let net = reactor.net.estimate_heap_size() as i64;
        let address_gossiper = reactor.address_gossiper.estimate_heap_size() as i64;
        let storage = reactor.storage.estimate_heap_size() as i64;
        let contract_runtime = reactor.contract_runtime.estimate_heap_size() as i64;
        let api_server = reactor.api_server.estimate_heap_size() as i64;
        let chainspec_loader = reactor.chainspec_loader.estimate_heap_size() as i64;
        let consensus = reactor.consensus.estimate_heap_size() as i64;
        let deploy_acceptor = reactor.deploy_acceptor.estimate_heap_size() as i64;
        let deploy_fetcher = reactor.deploy_fetcher.estimate_heap_size() as i64;
        let deploy_gossiper = reactor.deploy_gossiper.estimate_heap_size() as i64;
        let deploy_buffer = reactor.deploy_buffer.estimate_heap_size() as i64;
        let block_executor = reactor.block_executor.estimate_heap_size() as i64;
        let proto_block_validator = reactor.proto_block_validator.estimate_heap_size() as i64;

        let linear_chain = reactor.linear_chain.estimate_heap_size() as i64;

        let total = metrics
            + net
            + address_gossiper
            + storage
            + contract_runtime
            + api_server
            + chainspec_loader
            + consensus
            + deploy_acceptor
            + deploy_fetcher
            + deploy_gossiper
            + deploy_buffer
            + block_executor
            + proto_block_validator
            + linear_chain;

        self.mem_total.set(total);
        self.mem_metrics.set(metrics);
        self.mem_net.set(net);
        self.mem_address_gossiper.set(address_gossiper);
        self.mem_storage.set(storage);
        self.mem_contract_runtime.set(contract_runtime);
        self.mem_api_server.set(api_server);
        self.mem_chainspec_loader.set(chainspec_loader);
        self.mem_consensus.set(consensus);
        self.mem_deploy_acceptor.set(deploy_acceptor);
        self.mem_deploy_fetcher.set(deploy_fetcher);
        self.mem_deploy_gossiper.set(deploy_gossiper);
        self.mem_deploy_buffer.set(deploy_buffer);
        self.mem_block_executor.set(block_executor);
        self.mem_proto_block_validator.set(proto_block_validator);
        self.mem_linear_chain.set(linear_chain);

        // Stop the timer explicitly, don't count logging.
        drop(timer);

        debug!(%total,
               %net,
               %address_gossiper,
               %storage,
               %contract_runtime,
               %api_server,
               %chainspec_loader,
               %consensus,
               %deploy_acceptor,
               %deploy_fetcher,
               %deploy_gossiper,
               %deploy_buffer,
               %block_executor,
               %proto_block_validator,
               %linear_chain,
               "Collected new set of memory metrics.");
    }
}

impl Drop for MemoryMetrics {
    fn drop(&mut self) {
        self.registry
            .unregister(Box::new(self.mem_total.clone()))
            .expect("did not expect deregistering mem_total, to fail");
        self.registry
            .unregister(Box::new(self.mem_metrics.clone()))
            .expect("did not expect deregistering mem_metrics, to fail");
        self.registry
            .unregister(Box::new(self.mem_net.clone()))
            .expect("did not expect deregistering mem_net, to fail");
        self.registry
            .unregister(Box::new(self.mem_address_gossiper.clone()))
            .expect("did not expect deregistering mem_address_gossiper, to fail");
        self.registry
            .unregister(Box::new(self.mem_storage.clone()))
            .expect("did not expect deregistering mem_storage, to fail");
        self.registry
            .unregister(Box::new(self.mem_contract_runtime.clone()))
            .expect("did not expect deregistering mem_contract_runtime, to fail");
        self.registry
            .unregister(Box::new(self.mem_api_server.clone()))
            .expect("did not expect deregistering mem_api_server, to fail");
        self.registry
            .unregister(Box::new(self.mem_chainspec_loader.clone()))
            .expect("did not expect deregistering mem_chainspec_loader, to fail");
        self.registry
            .unregister(Box::new(self.mem_consensus.clone()))
            .expect("did not expect deregistering mem_consensus, to fail");
        self.registry
            .unregister(Box::new(self.mem_deploy_acceptor.clone()))
            .expect("did not expect deregistering mem_deploy_acceptor, to fail");
        self.registry
            .unregister(Box::new(self.mem_deploy_fetcher.clone()))
            .expect("did not expect deregistering mem_deploy_fetcher, to fail");
        self.registry
            .unregister(Box::new(self.mem_deploy_gossiper.clone()))
            .expect("did not expect deregistering mem_deploy_gossiper, to fail");
        self.registry
            .unregister(Box::new(self.mem_deploy_buffer.clone()))
            .expect("did not expect deregistering mem_deploy_buffer, to fail");
        self.registry
            .unregister(Box::new(self.mem_block_executor.clone()))
            .expect("did not expect deregistering mem_block_executor, to fail");
        self.registry
            .unregister(Box::new(self.mem_proto_block_validator.clone()))
            .expect("did not expect deregistering mem_proto_block_validator, to fail");
        self.registry
            .unregister(Box::new(self.mem_linear_chain.clone()))
            .expect("did not expect deregistering mem_linear_chain, to fail");
    }
}
