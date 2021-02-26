use std::env;

use datasize::DataSize;
use prometheus::{self, Histogram, HistogramOpts, IntGauge, Registry};
use tracing::{debug, warn};

use crate::components::network::ENABLE_LIBP2P_NET_ENV_VAR;

use super::Reactor;

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
    /// Estimated heap memory usage of rpc server component.
    mem_rpc_server: IntGauge,
    /// Estimated heap memory usage of rest server component.
    mem_rest_server: IntGauge,
    /// Estimated heap memory usage of event stream server component.
    mem_event_stream_server: IntGauge,
    /// Estimated heap memory usage of chainspec loader component.
    mem_chainspec_loader: IntGauge,
    /// Estimated heap memory usage of consensus component.
    mem_consensus: IntGauge,
    /// Estimated heap memory usage of deploy fetcher component.
    mem_deploy_fetcher: IntGauge,
    /// Estimated heap memory usage of deploy gossiper component.
    mem_deploy_gossiper: IntGauge,
    /// Estimated heap memory usage of block_proposer component.
    mem_block_proposer: IntGauge,
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
        let mem_rpc_server = IntGauge::new("mem_rpc_server", "rpc_server memory usage in bytes")?;
        let mem_rest_server =
            IntGauge::new("mem_rest_server", "mem_rest_server memory usage in bytes")?;
        let mem_event_stream_server = IntGauge::new(
            "mem_event_stream_server",
            "mem_event_stream_server memory usage in bytes",
        )?;
        let mem_chainspec_loader = IntGauge::new(
            "mem_chainspec_loader",
            "chainspec_loader memory usage in bytes",
        )?;
        let mem_consensus = IntGauge::new("mem_consensus", "consensus memory usage in bytes")?;
        let mem_deploy_fetcher =
            IntGauge::new("mem_deploy_fetcher", "deploy_fetcher memory usage in bytes")?;
        let mem_deploy_gossiper = IntGauge::new(
            "mem_deploy_gossiper",
            "deploy_gossiper memory usage in bytes",
        )?;
        let mem_block_proposer =
            IntGauge::new("mem_block_proposer", "block_proposer memory usage in bytes")?;
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
            mem_rpc_server,
            mem_rest_server,
            mem_event_stream_server,
            mem_chainspec_loader,
            mem_consensus,
            mem_deploy_fetcher,
            mem_deploy_gossiper,
            mem_block_proposer,
            mem_block_executor,
            mem_proto_block_validator,
            mem_linear_chain,
            mem_estimator_runtime_s,
            registry,
        })
    }

    /// Estimates memory usage and updates metrics.
    pub(super) fn estimate(&self, reactor: &Reactor) {
        let timer = self.mem_estimator_runtime_s.start_timer();

        let metrics = reactor.metrics.estimate_heap_size() as i64;
        let net = if env::var(ENABLE_LIBP2P_NET_ENV_VAR).is_ok() {
            reactor.network.estimate_heap_size() as i64
        } else {
            reactor.small_network.estimate_heap_size() as i64
        };
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
        let block_executor = reactor.block_executor.estimate_heap_size() as i64;
        let proto_block_validator = reactor.proto_block_validator.estimate_heap_size() as i64;

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
            + block_executor
            + proto_block_validator
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
        self.mem_block_executor.set(block_executor);
        self.mem_proto_block_validator.set(proto_block_validator);
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
            .unwrap_or_else(|err| warn!(%err, "did not expect deregistering mem_total, to fail"));
        self.registry
            .unregister(Box::new(self.mem_metrics.clone()))
            .unwrap_or_else(|err| warn!(%err, "did not expect deregistering mem_metrics, to fail"));
        self.registry
            .unregister(Box::new(self.mem_net.clone()))
            .unwrap_or_else(|err| warn!(%err, "did not expect deregistering mem_net, to fail"));
        self.registry
            .unregister(Box::new(self.mem_address_gossiper.clone()))
            .unwrap_or_else(
                |err| warn!(%err, "did not expect deregistering mem_address_gossiper, to fail"),
            );
        self.registry
            .unregister(Box::new(self.mem_storage.clone()))
            .unwrap_or_else(|err| warn!(%err, "did not expect deregistering mem_storage, to fail"));
        self.registry
            .unregister(Box::new(self.mem_contract_runtime.clone()))
            .unwrap_or_else(
                |err| warn!(%err, "did not expect deregistering mem_contract_runtime, to fail"),
            );
        self.registry
            .unregister(Box::new(self.mem_rpc_server.clone()))
            .unwrap_or_else(
                |err| warn!(%err, "did not expect deregistering mem_rpc_server, to fail"),
            );
        self.registry
            .unregister(Box::new(self.mem_rest_server.clone()))
            .expect("did not expect deregistering mem_rest_server, to fail");
        self.registry
            .unregister(Box::new(self.mem_event_stream_server.clone()))
            .unwrap_or_else(
                |err| warn!(%err, "did not expect deregistering mem_event_stream_server, to fail"),
            );
        self.registry
            .unregister(Box::new(self.mem_chainspec_loader.clone()))
            .unwrap_or_else(
                |err| warn!(%err, "did not expect deregistering mem_chainspec_loader, to fail"),
            );
        self.registry
            .unregister(Box::new(self.mem_consensus.clone()))
            .unwrap_or_else(
                |err| warn!(%err, "did not expect deregistering mem_consensus, to fail"),
            );
        self.registry
            .unregister(Box::new(self.mem_deploy_fetcher.clone()))
            .unwrap_or_else(
                |err| warn!(%err, "did not expect deregistering mem_deploy_fetcher, to fail"),
            );
        self.registry
            .unregister(Box::new(self.mem_deploy_gossiper.clone()))
            .unwrap_or_else(
                |err| warn!(%err, "did not expect deregistering mem_deploy_gossiper, to fail"),
            );
        self.registry
            .unregister(Box::new(self.mem_block_proposer.clone()))
            .unwrap_or_else(
                |err| warn!(%err, "did not expect deregistering mem_block_proposer, to fail"),
            );
        self.registry
            .unregister(Box::new(self.mem_block_executor.clone()))
            .unwrap_or_else(
                |err| warn!(%err, "did not expect deregistering mem_block_executor, to fail"),
            );
        self.registry
            .unregister(Box::new(self.mem_proto_block_validator.clone()))
            .unwrap_or_else(|err| warn!(%err, "did not expect deregistering mem_proto_block_validator, to fail"));
        self.registry
            .unregister(Box::new(self.mem_linear_chain.clone()))
            .unwrap_or_else(
                |err| warn!(%err, "did not expect deregistering mem_linear_chain, to fail"),
            );
    }
}
