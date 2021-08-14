mod error;
mod event;
mod operations;
mod state;

use std::{convert::Infallible, fmt::Debug, path::PathBuf};

use datasize::DataSize;
use prometheus::Registry;

use crate::{
    components::{contract_runtime, Component},
    effect::{EffectBuilder, EffectOptionExt, Effects},
    fatal,
    types::{BlockHash, BlockHeader, Chainspec},
    NodeRng,
};

use crate::reactor::joiner::JoinerEvent;
pub use event::Event;
pub use state::State;

#[derive(DataSize, Debug)]
pub(crate) struct LinearChainSync {
    state: State,

    chainspec: Chainspec,
    lmdb_path: PathBuf,
    contract_runtime_config: contract_runtime::Config,
    #[data_size(skip)]
    registry: Registry,
}

impl LinearChainSync {
    pub fn new(
        chainspec: &Chainspec,
        init_hash: Option<BlockHash>,
        lmdb_path: PathBuf,
        contract_runtime_config: contract_runtime::Config,
        registry: Registry,
    ) -> Self {
        let state = if let Some(block_hash) = init_hash {
            State::NotStarted(block_hash)
        } else {
            State::NotGoingToSync
        };
        LinearChainSync {
            chainspec: chainspec.clone(),
            state,
            lmdb_path,
            contract_runtime_config,
            registry,
        }
    }

    /// Returns `true` if we have finished syncing linear chain.
    pub fn is_synced(&self) -> bool {
        matches!(self.state, State::Done(_) | State::NotGoingToSync)
    }

    pub fn into_maybe_latest_block_header(self) -> Option<BlockHeader> {
        match self.state {
            State::Done(latest_block_header) => Some(*latest_block_header),
            State::NotGoingToSync | State::NotStarted(_) | State::Syncing => None,
        }
    }
}

impl Component<JoinerEvent> for LinearChainSync {
    type Event = Event;
    type ConstructionError = Infallible;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<JoinerEvent>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Finish(block_header) => {
                self.state = State::Done(block_header);
                Effects::new()
            }
            Event::Start => match self.state {
                State::NotStarted(init_hash) => {
                    self.state = State::Syncing;
                    let chainspec = self.chainspec.clone();
                    (async move {
                        match operations::run_fast_sync_task(effect_builder, init_hash, chainspec)
                            .await
                        {
                            Ok(block_header) => Some(Event::Finish(Box::new(block_header))),
                            Err(error) => {
                                fatal!(effect_builder, "{:?}", error).await;
                                None
                            }
                        }
                    })
                    .map_some(std::convert::identity)
                }
                _ => Effects::new(),
            },
        }
    }
}
