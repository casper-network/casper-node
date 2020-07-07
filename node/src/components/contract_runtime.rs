//! Contract Runtime component.
use std::{
    fmt::{Debug, Display},
    path::PathBuf,
    sync::Arc,
};

use lmdb::DatabaseFlags;
use rand::Rng;
use serde::{Deserialize, Serialize};

use crate::contract_core::engine_state::{EngineConfig, EngineState};
use crate::contract_shared::page_size;
use crate::contract_storage::protocol_data_store::lmdb::LmdbProtocolDataStore;
use crate::contract_storage::{
    global_state::lmdb::LmdbGlobalState, transaction_source::lmdb::LmdbEnvironment,
    trie_store::lmdb::LmdbTrieStore,
};

use crate::components::Component;
use crate::effect::{Effect, EffectBuilder, Multiple};

/// The contract runtime components.
pub(crate) struct ContractRuntime {
    #[allow(dead_code)]
    engine_state: EngineState<LmdbGlobalState>,
}

impl Debug for ContractRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ContractRuntime").finish()
    }
}

/// Contract runtime message used by the pinger.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Message;

/// Pinger component event.
#[derive(Debug)]
pub enum Event {
    /// Foo
    Foo,
    /// Bar
    Bar,
}

impl Display for Event {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Event::Foo => write!(f, "foo"),
            Event::Bar => write!(f, "bar"),
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
            Event::Foo => todo!("foo"),
            Event::Bar => todo!("bar"),
        }
    }
}

/// Builds and returns engine global state
fn get_engine_state(
    data_dir: PathBuf,
    map_size: usize,
    engine_config: EngineConfig,
) -> EngineState<LmdbGlobalState> {
    let environment = {
        let ret = LmdbEnvironment::new(&data_dir, map_size).expect("should have lmdb environment");
        Arc::new(ret)
    };

    let trie_store = {
        let ret = LmdbTrieStore::new(&environment, None, DatabaseFlags::empty())
            .expect("should have trie store");
        Arc::new(ret)
    };

    let protocol_data_store = {
        let ret = LmdbProtocolDataStore::new(&environment, None, DatabaseFlags::empty())
            .expect("should have protocol data store");
        Arc::new(ret)
    };

    let global_state = LmdbGlobalState::empty(environment, trie_store, protocol_data_store)
        .expect("should have global state");

    EngineState::new(global_state, engine_config)
}

impl ContractRuntime {
    /// Create and initialize a new pinger.
    pub(crate) fn new<REv: From<Event> + Send>(
        _effect_builder: EffectBuilder<REv>,
    ) -> (Self, Multiple<Effect<Event>>) {
        let engine_config = EngineConfig::new()
            .with_use_system_contracts(false)
            .with_enable_bonding(false);

        let engine_state = get_engine_state(
            PathBuf::from("/tmp"),
            page_size::get_page_size().expect("should get page size"),
            engine_config,
        );

        let contract_runtime = ContractRuntime { engine_state };

        let init = Multiple::new();

        (contract_runtime, init)
    }
}
