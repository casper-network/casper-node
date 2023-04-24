use datasize::DataSize;
use prometheus::{self, Histogram, IntGauge, Registry};
use tracing::debug;

use super::MainReactor;
use crate::utils::registered_metric::{RegisteredMetric, RegistryExt};

/// Metrics for estimated heap memory usage for the main reactor.
#[derive(Debug)]
pub(super) struct MemoryMetrics {
    mem_total: RegisteredMetric<IntGauge>,
    mem_metrics: RegisteredMetric<IntGauge>,
    mem_net: RegisteredMetric<IntGauge>,
    mem_address_gossiper: RegisteredMetric<IntGauge>,
    mem_storage: RegisteredMetric<IntGauge>,
    mem_contract_runtime: RegisteredMetric<IntGauge>,
    mem_rpc_server: RegisteredMetric<IntGauge>,
    mem_rest_server: RegisteredMetric<IntGauge>,
    mem_event_stream_server: RegisteredMetric<IntGauge>,
    mem_consensus: RegisteredMetric<IntGauge>,
    mem_deploy_gossiper: RegisteredMetric<IntGauge>,
    mem_finality_signature_gossiper: RegisteredMetric<IntGauge>,
    mem_block_gossiper: RegisteredMetric<IntGauge>,
    mem_deploy_buffer: RegisteredMetric<IntGauge>,
    mem_block_validator: RegisteredMetric<IntGauge>,
    mem_sync_leaper: RegisteredMetric<IntGauge>,
    mem_deploy_acceptor: RegisteredMetric<IntGauge>,
    mem_block_synchronizer: RegisteredMetric<IntGauge>,
    mem_block_accumulator: RegisteredMetric<IntGauge>,
    mem_fetchers: RegisteredMetric<IntGauge>,
    mem_diagnostics_port: RegisteredMetric<IntGauge>,
    mem_upgrade_watcher: RegisteredMetric<IntGauge>,
    /// Histogram detailing how long it took to measure memory usage.
    mem_estimator_runtime_s: RegisteredMetric<Histogram>,
}

impl MemoryMetrics {
    /// Initializes a new set of memory metrics.
    pub(super) fn new(registry: Registry) -> Result<Self, prometheus::Error> {
        let mem_total = registry.new_int_gauge("mem_total", "total memory usage in bytes")?;
        let mem_metrics = registry.new_int_gauge("mem_metrics", "metrics memory usage in bytes")?;
        let mem_net = registry.new_int_gauge("mem_net", "network memory usage in bytes")?;
        let mem_address_gossiper = registry.new_int_gauge(
            "mem_address_gossiper",
            "address_gossiper memory usage in bytes",
        )?;
        let mem_storage = registry.new_int_gauge("mem_storage", "storage memory usage in bytes")?;
        let mem_contract_runtime = registry.new_int_gauge(
            "mem_contract_runtime",
            "contract runtime memory usage in bytes",
        )?;
        let mem_rpc_server =
            registry.new_int_gauge("mem_rpc_server", "rpc server memory usage in bytes")?;
        let mem_rest_server =
            registry.new_int_gauge("mem_rest_server", "rest server memory usage in bytes")?;
        let mem_event_stream_server = registry.new_int_gauge(
            "mem_event_stream_server",
            "event stream server memory usage in bytes",
        )?;
        let mem_consensus =
            registry.new_int_gauge("mem_consensus", "consensus memory usage in bytes")?;
        let mem_fetchers =
            registry.new_int_gauge("mem_fetchers", "combined fetcher memory usage in bytes")?;
        let mem_deploy_gossiper = registry.new_int_gauge(
            "mem_deploy_gossiper",
            "deploy gossiper memory usage in bytes",
        )?;
        let mem_finality_signature_gossiper = registry.new_int_gauge(
            "mem_finality_signature_gossiper",
            "finality signature gossiper memory usage in bytes",
        )?;
        let mem_block_gossiper =
            registry.new_int_gauge("mem_block_gossiper", "block gossiper memory usage in bytes")?;
        let mem_deploy_buffer =
            registry.new_int_gauge("mem_deploy_buffer", "deploy buffer memory usage in bytes")?;
        let mem_block_validator = registry.new_int_gauge(
            "mem_block_validator",
            "block validator memory usage in bytes",
        )?;
        let mem_sync_leaper =
            registry.new_int_gauge("mem_sync_leaper", "sync leaper memory usage in bytes")?;
        let mem_deploy_acceptor = registry.new_int_gauge(
            "mem_deploy_acceptor",
            "deploy acceptor memory usage in bytes",
        )?;
        let mem_block_synchronizer = registry.new_int_gauge(
            "mem_block_synchronizer",
            "block synchronizer memory usage in bytes",
        )?;
        let mem_block_accumulator = registry.new_int_gauge(
            "mem_block_accumulator",
            "block accumulator memory usage in bytes",
        )?;
        let mem_diagnostics_port = registry.new_int_gauge(
            "mem_diagnostics_port",
            "diagnostics port memory usage in bytes",
        )?;
        let mem_upgrade_watcher = registry.new_int_gauge(
            "mem_upgrade_watcher",
            "upgrade watcher memory usage in bytes",
        )?;
        let mem_estimator_runtime_s = registry.new_histogram(
            "mem_estimator_runtime_s",
            "time in seconds to estimate memory usage",
            prometheus::exponential_buckets(0.000_000_004, 2.0, 32)?,
        )?;

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
            mem_consensus,
            mem_fetchers,
            mem_deploy_gossiper,
            mem_finality_signature_gossiper,
            mem_block_gossiper,
            mem_deploy_buffer,
            mem_block_validator,
            mem_sync_leaper,
            mem_deploy_acceptor,
            mem_block_synchronizer,
            mem_block_accumulator,
            mem_diagnostics_port,
            mem_upgrade_watcher,
            mem_estimator_runtime_s,
        })
    }

    /// Estimates memory usage and updates metrics.
    pub(super) fn estimate(&self, reactor: &MainReactor) {
        let timer = self.mem_estimator_runtime_s.start_timer();

        let metrics = reactor.metrics.estimate_heap_size() as i64;
        let network = reactor.net.estimate_heap_size() as i64;
        let address_gossiper = reactor.address_gossiper.estimate_heap_size() as i64;
        let storage = reactor.storage.estimate_heap_size() as i64;
        let contract_runtime = reactor.contract_runtime.estimate_heap_size() as i64;
        let rpc_server = reactor.rpc_server.estimate_heap_size() as i64;
        let rest_server = reactor.rest_server.estimate_heap_size() as i64;
        let event_stream_server = reactor.event_stream_server.estimate_heap_size() as i64;
        let consensus = reactor.consensus.estimate_heap_size() as i64;
        let fetchers = reactor.fetchers.estimate_heap_size() as i64;
        let deploy_gossiper = reactor.deploy_gossiper.estimate_heap_size() as i64;
        let finality_signature_gossiper =
            reactor.finality_signature_gossiper.estimate_heap_size() as i64;
        let block_gossiper = reactor.block_gossiper.estimate_heap_size() as i64;
        let deploy_buffer = reactor.deploy_buffer.estimate_heap_size() as i64;
        let block_validator = reactor.block_validator.estimate_heap_size() as i64;
        let sync_leaper = reactor.sync_leaper.estimate_heap_size() as i64;
        let deploy_acceptor = reactor.deploy_acceptor.estimate_heap_size() as i64;
        let block_synchronizer = reactor.block_synchronizer.estimate_heap_size() as i64;
        let block_accumulator = reactor.block_accumulator.estimate_heap_size() as i64;
        let diagnostics_port = reactor.diagnostics_port.estimate_heap_size() as i64;
        let upgrade_watcher = reactor.upgrade_watcher.estimate_heap_size() as i64;

        let total = metrics
            + network
            + address_gossiper
            + storage
            + contract_runtime
            + rpc_server
            + rest_server
            + event_stream_server
            + consensus
            + fetchers
            + deploy_gossiper
            + finality_signature_gossiper
            + block_gossiper
            + deploy_buffer
            + block_validator
            + sync_leaper
            + deploy_acceptor
            + block_synchronizer
            + block_accumulator
            + diagnostics_port
            + upgrade_watcher;

        self.mem_net.set(network);
        self.mem_address_gossiper.set(address_gossiper);
        self.mem_storage.set(storage);
        self.mem_contract_runtime.set(contract_runtime);
        self.mem_rpc_server.set(rpc_server);
        self.mem_rest_server.set(rest_server);
        self.mem_event_stream_server.set(event_stream_server);
        self.mem_consensus.set(consensus);
        self.mem_fetchers.set(fetchers);
        self.mem_deploy_gossiper.set(deploy_gossiper);
        self.mem_finality_signature_gossiper
            .set(finality_signature_gossiper);
        self.mem_block_gossiper.set(block_gossiper);
        self.mem_deploy_buffer.set(deploy_buffer);
        self.mem_block_validator.set(block_validator);
        self.mem_sync_leaper.set(sync_leaper);
        self.mem_deploy_acceptor.set(deploy_acceptor);
        self.mem_block_synchronizer.set(block_synchronizer);
        self.mem_block_accumulator.set(block_accumulator);
        self.mem_diagnostics_port.set(diagnostics_port);
        self.mem_upgrade_watcher.set(upgrade_watcher);

        self.mem_total.set(total);
        self.mem_metrics.set(metrics);

        // Stop the timer explicitly, don't count logging.
        let duration_s = timer.stop_and_record();

        debug!(%total,
               %duration_s,
               %metrics,
               %network,
               %address_gossiper,
               %storage,
               %contract_runtime,
               %rpc_server,
               %rest_server,
               %event_stream_server,
               %consensus,
               %fetchers,
               %deploy_gossiper,
               %finality_signature_gossiper,
               %block_gossiper,
               %deploy_buffer,
               %block_validator,
               %sync_leaper,
               %deploy_acceptor,
               %block_synchronizer,
               %block_accumulator,
               %diagnostics_port,
               %upgrade_watcher,
               "Collected new set of memory metrics.");
    }
}
