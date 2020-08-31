//! Reactor used to join the network.

use std::{
    fmt::{self, Display, Formatter},
    path::PathBuf,
};

use derive_more::From;
use prometheus::Registry;
use rand::{CryptoRng, Rng};

use crate::{
    components::{
        chainspec_loader::ChainspecLoader, contract_runtime::ContractRuntime, small_network,
        small_network::SmallNetwork, storage::Storage,
    },
    effect::{EffectBuilder, Effects},
    reactor::{self, error::Error, initializer, validator, EventQueueHandle, Message},
    utils::WithDir,
};

/// Top-level event for the reactor.
#[derive(Debug, From)]
#[must_use]
pub enum Event {
    /// Network event.
    #[from]
    Network(small_network::Event<Message>),
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Network(event) => write!(f, "network: {}", event),
        }
    }
}

/// Joining node reactor.
#[derive(Debug)]
pub struct Reactor {
    pub(super) root: PathBuf,
    pub(super) net: SmallNetwork<Event, Message>,
    pub(super) config: validator::Config,
    pub(super) chainspec_loader: ChainspecLoader,
    pub(super) storage: Storage,
    pub(super) contract_runtime: ContractRuntime,
}

impl<R: Rng + CryptoRng + ?Sized> reactor::Reactor<R> for Reactor {
    type Event = Event; // Temporary; u8 instead of () so that it implements Display

    // The "configuration" is in fact the whole state of the initializer reactor, which we
    // deconstruct and reuse.
    type Config = WithDir<initializer::Reactor>;
    type Error = Error;

    fn new(
        initializer: Self::Config,
        _registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut R,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let (root, initializer) = initializer.into_parts();

        let initializer::Reactor {
            config,
            chainspec_loader,
            storage,
            contract_runtime,
        } = initializer;

        let (net, _net_effects) = SmallNetwork::new(
            event_queue,
            WithDir::new(root.clone(), config.validator_net.clone()),
        )?;

        Ok((
            Self {
                net,
                root,
                config,
                chainspec_loader,
                storage,
                contract_runtime,
            },
            Default::default(),
        ))
    }

    fn dispatch_event(
        &mut self,
        _effect_builder: EffectBuilder<Self::Event>,
        _rng: &mut R,
        _event: Self::Event,
    ) -> Effects<Self::Event> {
        Default::default()
    }

    fn is_stopped(&mut self) -> bool {
        // TODO!
        true
    }
}
