//! Contract Runtime component.
mod config;
pub mod core;
pub mod shared;
pub mod storage;

use std::{
    fmt::{self, Debug, Display, Formatter},
    sync::Arc,
};

use derive_more::From;
use lmdb::DatabaseFlags;
use rand::{CryptoRng, Rng};
use thiserror::Error;
use tokio::task;
use tracing::trace;

use crate::{
    components::{
        contract_runtime::{
            core::engine_state::{EngineConfig, EngineState},
            shared::newtypes::CorrelationId,
            storage::{
                error::lmdb::Error as StorageLmdbError, global_state::lmdb::LmdbGlobalState,
                protocol_data_store::lmdb::LmdbProtocolDataStore,
                transaction_source::lmdb::LmdbEnvironment, trie_store::lmdb::LmdbTrieStore,
            },
        },
        Component,
    },
    effect::{requests::ContractRuntimeRequest, EffectBuilder, EffectExt, Effects},
    StorageConfig,
};
pub use config::Config;
use prometheus::{exponential_buckets, Histogram, HistogramOpts, Registry};
use std::time::Instant;

/// The contract runtime components.
pub(crate) struct ContractRuntime {
    engine_state: Arc<EngineState<LmdbGlobalState>>,
    metrics: Arc<ContractRuntimeMetrics>,
}

impl Debug for ContractRuntime {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContractRuntime").finish()
    }
}

/// Contract runtime component event.
#[derive(Debug, From)]
pub enum Event {
    /// A request made of the contract runtime component.
    #[from]
    Request(ContractRuntimeRequest),
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Request(request) => write!(f, "{}", request),
        }
    }
}

/// Metrics for the contract runtime component.
#[derive(Debug)]
pub struct ContractRuntimeMetrics {
    run_execute: Histogram,
    apply_effect: Histogram,
    commit_upgrade: Histogram,
    run_query: Histogram,
}

/// Value of upper bound of histogram.
const EXPONENTIAL_BUCKET_START: f64 = 0.01;
/// Multiplier of previous upper bound for next bound.
const EXPONENTIAL_BUCKET_FACTOR: f64 = 2.0;
/// Bucket count, with last going to +Inf.
const EXPONENTIAL_BUCKET_COUNT: usize = 6;

const RUN_EXECUTE_NAME: &str = "contract_runtime_run_execute";
const RUN_EXECUTE_HELP: &str = "tracking run of engine_state.run_execute.";
const APPLY_EFFECT_NAME: &str = "contract_runtime_apply_commit";
const APPLY_EFFECT_HELP: &str = "tracking run of engine_state.apply_effect.";
const RUN_QUERY_NAME: &str = "contract_runtime_run_query";
const RUN_QUERY_HELP: &str = "tracking run of engine_state.run_query.";
const COMMIT_UPGRADE_NAME: &str = "contract_runtime_commit_upgrade";
const COMMIT_UPGRADE_HELP: &str = "tracking run of engine_state.commit_upgrade";

/// Create prometheus Histogram and register.
fn register_histogram_metric(
    registry: &Registry,
    metric_name: &str,
    metric_help: &str,
) -> Result<Histogram, prometheus::Error> {
    let common_buckets = exponential_buckets(
        EXPONENTIAL_BUCKET_START,
        EXPONENTIAL_BUCKET_FACTOR,
        EXPONENTIAL_BUCKET_COUNT,
    )?;
    let histogram_opts = HistogramOpts::new(metric_name, metric_help).buckets(common_buckets);
    let histogram = Histogram::with_opts(histogram_opts)?;
    registry.register(Box::new(histogram.clone()))?;
    Ok(histogram)
}

impl ContractRuntimeMetrics {
    /// Constructor of metrics which creates and registers metrics objects for use.
    fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        Ok(ContractRuntimeMetrics {
            run_execute: register_histogram_metric(registry, RUN_EXECUTE_NAME, RUN_EXECUTE_HELP)?,
            apply_effect: register_histogram_metric(
                registry,
                APPLY_EFFECT_NAME,
                APPLY_EFFECT_HELP,
            )?,
            run_query: register_histogram_metric(registry, RUN_QUERY_NAME, RUN_QUERY_HELP)?,
            commit_upgrade: register_histogram_metric(
                registry,
                COMMIT_UPGRADE_NAME,
                COMMIT_UPGRADE_HELP,
            )?,
        })
    }
}

impl<REv, R> Component<REv, R> for ContractRuntime
where
    REv: From<Event> + Send,
    R: Rng + CryptoRng + ?Sized,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        _rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(ContractRuntimeRequest::CommitGenesis {
                chainspec,
                responder,
            }) => {
                let result = self.engine_state.commit_genesis(*chainspec);
                responder.respond(result).ignore()
            }
            Event::Request(ContractRuntimeRequest::Execute {
                execute_request,
                responder,
            }) => {
                trace!(?execute_request, "execute");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        let start = Instant::now();
                        let execution_result =
                            engine_state.run_execute(correlation_id, execute_request);
                        metrics.run_execute.observe(start.elapsed().as_secs_f64());
                        execution_result
                    })
                    .await
                    .expect("should run");
                    trace!(?result, "execute result");
                    responder.respond(result).await
                }
                .ignore()
            }
            Event::Request(ContractRuntimeRequest::Commit {
                protocol_version,
                pre_state_hash,
                effects,
                responder,
            }) => {
                trace!(?protocol_version, ?pre_state_hash, ?effects, "commit");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        let start = Instant::now();
                        let apply_result = engine_state.apply_effect(
                            correlation_id,
                            protocol_version,
                            pre_state_hash,
                            effects,
                        );
                        metrics.apply_effect.observe(start.elapsed().as_secs_f64());
                        apply_result
                    })
                    .await
                    .expect("should run");
                    trace!(?result, "commit result");
                    responder.respond(result).await
                }
                .ignore()
            }
            Event::Request(ContractRuntimeRequest::Upgrade {
                upgrade_config,
                responder,
            }) => {
                trace!(?upgrade_config, "upgrade");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        let start = Instant::now();
                        let result = engine_state.commit_upgrade(correlation_id, upgrade_config);
                        metrics
                            .commit_upgrade
                            .observe(start.elapsed().as_secs_f64());
                        result
                    })
                    .await
                    .expect("should run");
                    trace!(?result, "upgrade result");
                    responder.respond(result).await
                }
                .ignore()
            }
            Event::Request(ContractRuntimeRequest::Query {
                query_request,
                responder,
            }) => {
                trace!(?query_request, "query");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        let start = Instant::now();
                        let result = engine_state.run_query(correlation_id, query_request);
                        metrics.run_query.observe(start.elapsed().as_secs_f64());
                        result
                    })
                    .await
                    .expect("should run");
                    trace!(?result, "query result");
                    responder.respond(result).await
                }
                .ignore()
            }
        }
    }
}

/// Error returned from mis-configuring the contract runtime component.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Error initializing the LMDB environment.
    #[error("failed to initialize LMDB environment for contract runtime: {0}")]
    Lmdb(#[from] StorageLmdbError),
    /// Error initializing metrics.
    #[error("failed to initialize metrics for contract runtime: {0}")]
    Prometheus(#[from] prometheus::Error),
}

impl ContractRuntime {
    pub(crate) fn new(
        storage_config: &StorageConfig,
        contract_runtime_config: Config,
        registry: &Registry,
    ) -> Result<Self, ConfigError> {
        let path = storage_config.path();
        let environment = Arc::new(LmdbEnvironment::new(
            path.as_path(),
            contract_runtime_config.max_global_state_size(),
        )?);

        let trie_store = Arc::new(LmdbTrieStore::new(
            &environment,
            None,
            DatabaseFlags::empty(),
        )?);

        let protocol_data_store = Arc::new(LmdbProtocolDataStore::new(
            &environment,
            None,
            DatabaseFlags::empty(),
        )?);

        let global_state = LmdbGlobalState::empty(environment, trie_store, protocol_data_store)?;
        let engine_config = EngineConfig::new()
            .with_use_system_contracts(contract_runtime_config.use_system_contracts())
            .with_enable_bonding(contract_runtime_config.enable_bonding());

        let engine_state = Arc::new(EngineState::new(global_state, engine_config));

        let metrics = Arc::new(ContractRuntimeMetrics::new(registry)?);
        Ok(ContractRuntime {
            engine_state,
            metrics,
        })
    }
}
