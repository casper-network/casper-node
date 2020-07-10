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

use crate::{
    components::{
        contract_runtime::{
            core::engine_state::{EngineConfig, EngineState},
            storage::{
                error::lmdb::Error as StorageLmdbError, global_state::lmdb::LmdbGlobalState,
                protocol_data_store::lmdb::LmdbProtocolDataStore,
                transaction_source::lmdb::LmdbEnvironment, trie_store::lmdb::LmdbTrieStore,
            },
        },
        Component,
    },
    effect::{requests::ContractRuntimeRequest, Effect, EffectBuilder, EffectExt, Multiple},
    StorageConfig,
};

pub use config::Config;

/// The contract runtime components.
pub(crate) struct ContractRuntime {
    #[allow(dead_code)]
    engine_state: EngineState<LmdbGlobalState>,
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
    ) -> Multiple<Effect<Self::Event>> {
        match event {
            Event::Request(request) => match request {
                ContractRuntimeRequest::CommitGenesis {
                    chainspec,
                    responder,
                } => {
                    let result = self.engine_state.commit_genesis(*chainspec);
                    responder.respond(result).ignore()
                }
            },
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

        let engine_state = EngineState::new(global_state, engine_config);

        Ok(ContractRuntime { engine_state })
    }
}
