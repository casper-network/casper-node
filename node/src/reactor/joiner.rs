//! Reactor used to join the network.

use std::path::PathBuf;

use prometheus::Registry;
use rand::{CryptoRng, Rng};

use crate::{
    components::{
        chainspec_loader::ChainspecLoader, contract_runtime::ContractRuntime, storage::Storage,
    },
    effect::{EffectBuilder, Effects},
    reactor::{self, initializer, validator, EventQueueHandle},
    utils::WithDir,
};

/// Joining node reactor.
#[derive(Debug)]
pub struct Reactor {
    pub(super) root: PathBuf,
    pub(super) config: validator::Config,
    pub(super) chainspec_loader: ChainspecLoader,
    pub(super) storage: Storage,
    pub(super) contract_runtime: ContractRuntime,
}

impl<R: Rng + CryptoRng + ?Sized> reactor::Reactor<R> for Reactor {
    type Event = u8; // Temporary; u8 instead of () so that it implements Display

    // The "configuration" is in fact the whole state of the initializer reactor, which we
    // deconstruct and reuse.
    type Config = WithDir<initializer::Reactor>;
    type Error = prometheus::Error;

    fn new(
        initializer: Self::Config,
        _registry: &Registry,
        _event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut R,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let (root, initializer) = initializer.into_parts();

        let initializer::Reactor {
            config,
            chainspec_loader,
            storage,
            contract_runtime,
        } = initializer;

        Ok((
            Self {
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
}
