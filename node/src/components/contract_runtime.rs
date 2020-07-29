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
use rand::Rng;
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

/// The contract runtime components.
pub(crate) struct ContractRuntime {
    engine_state: Arc<EngineState<LmdbGlobalState>>,
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

impl<REv> Component<REv> for ContractRuntime
where
    REv: From<Event> + Send,
{
    type Event = Event;

    fn handle_event<R: Rng + ?Sized>(
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
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        engine_state.run_execute(correlation_id, execute_request)
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
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        engine_state.apply_effect(
                            correlation_id,
                            protocol_version,
                            pre_state_hash,
                            effects,
                        )
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
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        engine_state.commit_upgrade(correlation_id, upgrade_config)
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
                async move {
                    let correlation_id = CorrelationId::new();
                    let result = task::spawn_blocking(move || {
                        engine_state.run_query(correlation_id, query_request)
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
}

impl ContractRuntime {
    pub(crate) fn new(
        storage_config: &StorageConfig,
        contract_runtime_config: Config,
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

        Ok(ContractRuntime { engine_state })
    }
}
