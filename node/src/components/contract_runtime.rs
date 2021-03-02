//! Contract Runtime component.
mod config;
mod types;

pub use config::Config;
pub use types::{EraValidatorsRequest, ValidatorWeightsByEraIdRequest};

use std::{
    fmt::{self, Debug, Display, Formatter},
    sync::Arc,
    time::Instant,
};

use datasize::DataSize;
use derive_more::From;
use lmdb::DatabaseFlags;
use prometheus::{self, Histogram, HistogramOpts, Registry};
use serde::Serialize;
use thiserror::Error;
use tokio::task;
use tracing::{error, trace};

use casper_execution_engine::{
    core::engine_state::{
        genesis::GenesisResult, EngineConfig, EngineState, Error, GetEraValidatorsError,
        GetEraValidatorsRequest,
    },
    shared::newtypes::CorrelationId,
    storage::{
        error::lmdb::Error as StorageLmdbError, global_state::lmdb::LmdbGlobalState,
        protocol_data_store::lmdb::LmdbProtocolDataStore,
        transaction_source::lmdb::LmdbEnvironment, trie_store::lmdb::LmdbTrieStore,
    },
};
use casper_types::{system::auction::ValidatorWeights, ProtocolVersion};

use crate::{
    components::Component,
    effect::{requests::ContractRuntimeRequest, EffectBuilder, EffectExt, Effects},
    types::Chainspec,
    utils::WithDir,
    NodeRng, StorageConfig,
};

/// The contract runtime components.
#[derive(DataSize)]
pub struct ContractRuntime {
    engine_state: Arc<EngineState<LmdbGlobalState>>,
    metrics: Arc<ContractRuntimeMetrics>,
}

impl Debug for ContractRuntime {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ContractRuntime").finish()
    }
}

/// Contract runtime component event.
#[derive(Debug, From, Serialize)]
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
    commit_step: Histogram,
    get_balance: Histogram,
    get_validator_weights: Histogram,
    get_era_validators: Histogram,
    get_era_validator_weights_by_era_id: Histogram,
    get_bids: Histogram,
    missing_trie_keys: Histogram,
    put_trie: Histogram,
    read_trie: Histogram,
}

/// Value of upper bound of histogram.
const EXPONENTIAL_BUCKET_START: f64 = 0.01;

/// Multiplier of previous upper bound for next bound.
const EXPONENTIAL_BUCKET_FACTOR: f64 = 2.0;

/// Bucket count, with the last bucket going to +Inf which will not be included in the results.
/// - start = 0.01, factor = 2.0, count = 10
/// - start * factor ^ count = 0.01 * 2.0 ^ 10 = 10.24
/// - Values above 10.24 (f64 seconds here) will not fall in a bucket that is kept.
const EXPONENTIAL_BUCKET_COUNT: usize = 10;

const RUN_EXECUTE_NAME: &str = "contract_runtime_run_execute";
const RUN_EXECUTE_HELP: &str = "tracking run of engine_state.run_execute in seconds.";
const APPLY_EFFECT_NAME: &str = "contract_runtime_apply_commit";
const APPLY_EFFECT_HELP: &str = "tracking run of engine_state.apply_effect in seconds.";
const RUN_QUERY_NAME: &str = "contract_runtime_run_query";
const RUN_QUERY_HELP: &str = "tracking run of engine_state.run_query in seconds.";
const COMMIT_STEP_NAME: &str = "contract_runtime_commit_step";
const COMMIT_STEP_HELP: &str = "tracking run of engine_state.commit_step in seconds.";
const COMMIT_UPGRADE_NAME: &str = "contract_runtime_commit_upgrade";
const COMMIT_UPGRADE_HELP: &str = "tracking run of engine_state.commit_upgrade in seconds";
const GET_BALANCE_NAME: &str = "contract_runtime_get_balance";
const GET_BALANCE_HELP: &str = "tracking run of engine_state.get_balance in seconds.";
const GET_VALIDATOR_WEIGHTS_NAME: &str = "contract_runtime_get_validator_weights";
const GET_VALIDATOR_WEIGHTS_HELP: &str =
    "tracking run of engine_state.get_validator_weights in seconds.";
const GET_ERA_VALIDATORS_NAME: &str = "contract_runtime_get_era_validators";
const GET_ERA_VALIDATORS_HELP: &str = "tracking run of engine_state.get_era_validators in seconds.";
const GET_ERA_VALIDATORS_WEIGHT_BY_ERA_ID_NAME: &str =
    "contract_runtime_get_era_validator_weights_by_era_id";
const GET_ERA_VALIDATORS_WEIGHT_BY_ERA_ID_HELP: &str =
    "tracking run of engine_state.get_era_validator_weights_by_era_id in seconds.";
const GET_BIDS_NAME: &str = "contract_runtime_get_bids";
const GET_BIDS_HELP: &str = "tracking run of engine_state.get_bids in seconds.";
const READ_TRIE_NAME: &str = "contract_runtime_read_trie";
const READ_TRIE_HELP: &str = "tracking run of engine_state.read_trie in seconds.";
const PUT_TRIE_NAME: &str = "contract_runtime_put_trie";
const PUT_TRIE_HELP: &str = "tracking run of engine_state.put_trie in seconds.";
const MISSING_TRIE_KEYS_NAME: &str = "contract_runtime_missing_trie_keys";
const MISSING_TRIE_KEYS_HELP: &str = "tracking run of engine_state.missing_trie_keys in seconds.";

/// Create prometheus Histogram and register.
fn register_histogram_metric(
    registry: &Registry,
    metric_name: &str,
    metric_help: &str,
) -> Result<Histogram, prometheus::Error> {
    let common_buckets = prometheus::exponential_buckets(
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
            commit_step: register_histogram_metric(registry, COMMIT_STEP_NAME, COMMIT_STEP_HELP)?,
            commit_upgrade: register_histogram_metric(
                registry,
                COMMIT_UPGRADE_NAME,
                COMMIT_UPGRADE_HELP,
            )?,
            get_balance: register_histogram_metric(registry, GET_BALANCE_NAME, GET_BALANCE_HELP)?,
            get_validator_weights: register_histogram_metric(
                registry,
                GET_VALIDATOR_WEIGHTS_NAME,
                GET_VALIDATOR_WEIGHTS_HELP,
            )?,
            get_era_validators: register_histogram_metric(
                registry,
                GET_ERA_VALIDATORS_NAME,
                GET_ERA_VALIDATORS_HELP,
            )?,
            get_era_validator_weights_by_era_id: register_histogram_metric(
                registry,
                GET_ERA_VALIDATORS_WEIGHT_BY_ERA_ID_NAME,
                GET_ERA_VALIDATORS_WEIGHT_BY_ERA_ID_HELP,
            )?,
            get_bids: register_histogram_metric(registry, GET_BIDS_NAME, GET_BIDS_HELP)?,
            read_trie: register_histogram_metric(registry, READ_TRIE_NAME, READ_TRIE_HELP)?,
            put_trie: register_histogram_metric(registry, PUT_TRIE_NAME, PUT_TRIE_HELP)?,
            missing_trie_keys: register_histogram_metric(
                registry,
                MISSING_TRIE_KEYS_NAME,
                MISSING_TRIE_KEYS_HELP,
            )?,
        })
    }
}

impl<REv> Component<REv> for ContractRuntime
where
    REv: From<Event> + Send,
{
    type Event = Event;
    type ConstructionError = ConfigError;

    fn handle_event(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Request(ContractRuntimeRequest::GetProtocolData {
                protocol_version,
                responder,
            }) => {
                let result = self
                    .engine_state
                    .get_protocol_data(protocol_version)
                    .map(|inner| inner.map(Box::new));

                responder.respond(result).ignore()
            }
            Event::Request(ContractRuntimeRequest::CommitGenesis {
                chainspec,
                responder,
            }) => {
                let result = self.commit_genesis(chainspec);
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
                            engine_state.run_execute(correlation_id, *execute_request);
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
                state_root_hash,
                effects,
                responder,
            }) => {
                trace!(?state_root_hash, ?effects, "commit");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        let start = Instant::now();
                        let apply_result = engine_state.apply_effect(
                            correlation_id,
                            state_root_hash.into(),
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
                        let result = engine_state.commit_upgrade(correlation_id, *upgrade_config);
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
            Event::Request(ContractRuntimeRequest::GetBalance {
                balance_request,
                responder,
            }) => {
                trace!(?balance_request, "balance");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        let start = Instant::now();
                        let result = engine_state.get_purse_balance(
                            correlation_id,
                            balance_request.state_hash(),
                            balance_request.purse_uref(),
                        );
                        metrics.get_balance.observe(start.elapsed().as_secs_f64());
                        result
                    })
                    .await
                    .expect("should run");
                    trace!(?result, "balance result");
                    responder.respond(result).await
                }
                .ignore()
            }
            Event::Request(ContractRuntimeRequest::IsBonded {
                state_root_hash,
                era_id,
                protocol_version,
                public_key: validator_key,
                responder,
            }) => {
                trace!(era=%era_id, public_key = %validator_key, "is validator bonded request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                let request =
                    GetEraValidatorsRequest::new(state_root_hash.into(), protocol_version);
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        let start = Instant::now();
                        let era_validators =
                            engine_state.get_era_validators(correlation_id, request);
                        metrics
                            .get_validator_weights
                            .observe(start.elapsed().as_secs_f64());
                        era_validators
                    })
                    .await
                    .expect("should run");
                    trace!(?result, "is validator bonded result");
                    let is_bonded =
                        result.and_then(|validator_map| match validator_map.get(&era_id.0) {
                            None => Err(GetEraValidatorsError::EraValidatorsMissing),
                            Some(era_validators) => Ok(era_validators.contains_key(&validator_key)),
                        });
                    responder.respond(is_bonded).await
                }
                .ignore()
            }
            Event::Request(ContractRuntimeRequest::GetEraValidators { request, responder }) => {
                trace!(?request, "get era validators request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                // Increment the counter to track the amount of times GetEraValidators was
                // requested.
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        let start = Instant::now();
                        let era_validators =
                            engine_state.get_era_validators(correlation_id, request.into());
                        metrics
                            .get_era_validators
                            .observe(start.elapsed().as_secs_f64());
                        era_validators
                    })
                    .await
                    .expect("should run");
                    trace!(?result, "get era validators response");
                    responder.respond(result).await
                }
                .ignore()
            }
            Event::Request(ContractRuntimeRequest::GetValidatorWeightsByEraId {
                request,
                responder,
            }) => {
                trace!(?request, "get validator weights by era id request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                // Increment the counter to track the amount of times GetEraValidatorsByEraId was
                // requested.
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        let start = Instant::now();
                        let era_id = request.era_id().into();
                        let era_validators =
                            engine_state.get_era_validators(correlation_id, request.into());
                        let ret: Result<Option<ValidatorWeights>, GetEraValidatorsError> =
                            match era_validators {
                                Ok(era_validators) => {
                                    let validator_weights = era_validators.get(&era_id).cloned();
                                    Ok(validator_weights)
                                }
                                Err(GetEraValidatorsError::EraValidatorsMissing) => Ok(None),
                                Err(error) => Err(error),
                            };
                        metrics
                            .get_era_validator_weights_by_era_id
                            .observe(start.elapsed().as_secs_f64());
                        ret
                    })
                    .await
                    .expect("should run");
                    trace!(?result, "get validator weights by era id response");
                    responder.respond(result).await
                }
                .ignore()
            }
            Event::Request(ContractRuntimeRequest::GetBids {
                get_bids_request,
                responder,
            }) => {
                trace!(?get_bids_request, "get bids request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        let start = Instant::now();
                        let result = engine_state.get_bids(correlation_id, get_bids_request);
                        metrics.get_bids.observe(start.elapsed().as_secs_f64());
                        result
                    })
                    .await
                    .expect("should run");
                    trace!(?result, "get bids result");
                    responder.respond(result).await
                }
                .ignore()
            }
            Event::Request(ContractRuntimeRequest::Step {
                step_request,
                responder,
            }) => {
                trace!(?step_request, "step request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        let start = Instant::now();
                        let result = engine_state.commit_step(correlation_id, step_request);
                        metrics.commit_step.observe(start.elapsed().as_secs_f64());
                        result
                    })
                    .await
                    .expect("should run");
                    trace!(?result, "step response");
                    responder.respond(result).await
                }
                .ignore()
            }
            Event::Request(ContractRuntimeRequest::ReadTrie {
                trie_key,
                responder,
            }) => {
                trace!(?trie_key, "read_trie request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        let start = Instant::now();
                        let result = engine_state.read_trie(correlation_id, trie_key);
                        metrics.read_trie.observe(start.elapsed().as_secs_f64());
                        result
                    })
                    .await
                    .expect("should run");
                    let result = match result {
                        Ok(result) => result,
                        Err(error) => {
                            error!(?error, "read_trie_request");
                            None
                        }
                    };
                    trace!(?result, "read_trie response");
                    responder.respond(result).await
                }
                .ignore()
            }
            Event::Request(ContractRuntimeRequest::PutTrie { trie, responder }) => {
                trace!(?trie, "put_trie request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        let start = Instant::now();
                        let result = engine_state
                            .put_trie_and_find_missing_descendant_trie_keys(correlation_id, &*trie);
                        metrics.put_trie.observe(start.elapsed().as_secs_f64());
                        result
                    })
                    .await
                    .expect("should run");
                    trace!(?result, "put_trie response");
                    responder.respond(result).await
                }
                .ignore()
            }
            Event::Request(ContractRuntimeRequest::MissingTrieKeys {
                trie_key,
                responder,
            }) => {
                trace!(?trie_key, "missing_trie_keys request");
                let engine_state = Arc::clone(&self.engine_state);
                let metrics = Arc::clone(&self.metrics);
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        let start = Instant::now();
                        let result = engine_state.missing_trie_keys(correlation_id, trie_key);
                        metrics.read_trie.observe(start.elapsed().as_secs_f64());
                        result
                    })
                    .await
                    .expect("should run");
                    trace!(?result, "missing_trie_keys response");
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
        storage_config: WithDir<StorageConfig>,
        contract_runtime_config: &Config,
        registry: &Registry,
    ) -> Result<Self, ConfigError> {
        let path = storage_config.with_dir(storage_config.value().path.clone());
        let environment = Arc::new(LmdbEnvironment::new(
            path.as_path(),
            contract_runtime_config.max_global_state_size(),
            contract_runtime_config.max_readers(),
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
        let engine_config = EngineConfig::new();

        let engine_state = Arc::new(EngineState::new(global_state, engine_config));

        let metrics = Arc::new(ContractRuntimeMetrics::new(registry)?);
        Ok(ContractRuntime {
            engine_state,
            metrics,
        })
    }

    /// Commits a genesis using a chainspec
    fn commit_genesis(&self, chainspec: Arc<Chainspec>) -> Result<GenesisResult, Error> {
        let correlation_id = CorrelationId::new();
        let genesis_config_hash = chainspec.hash();
        let protocol_version = ProtocolVersion::from_parts(
            chainspec.protocol_config.version.major as u32,
            chainspec.protocol_config.version.minor as u32,
            chainspec.protocol_config.version.patch as u32,
        );
        // Transforms a chainspec into a valid genesis config for execution engine.
        let ee_config = chainspec.as_ref().into();
        self.engine_state.commit_genesis(
            correlation_id,
            genesis_config_hash.into(),
            protocol_version,
            &ee_config,
        )
    }
}
