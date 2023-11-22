use std::{
    fmt::{self, Debug, Formatter},
    sync::Arc,
};

use either::Either;
use futures::future::BoxFuture;
use prometheus::Registry;

use casper_types::{Chainspec, ChainspecRawBytes};

use super::network::NetworkedReactor;
use crate::{
    components::network::Identity as NetworkIdentity,
    effect::{EffectBuilder, Effects},
    failpoints::FailpointActivation,
    reactor::{EventQueueHandle, Finalize, Reactor},
    types::NodeId,
    NodeRng,
};

pub(crate) trait EventFilter<Ev>:
    FnMut(Ev) -> Either<Effects<Ev>, Ev> + Send + 'static
{
}
impl<Ev, T> EventFilter<Ev> for T where T: FnMut(Ev) -> Either<Effects<Ev>, Ev> + Send + 'static {}

/// A reactor wrapping an inner reactor, which has a hook into `Reactor::dispatch_event()` that
/// allows overriding or modifying event handling.
pub(crate) struct FilterReactor<R: Reactor> {
    reactor: R,
    filter: Box<dyn EventFilter<R::Event>>,
}

/// A filter that doesn't modify the behavior.
impl<R: Reactor> FilterReactor<R> {
    /// Sets the event filter.
    pub(crate) fn set_filter(&mut self, filter: impl EventFilter<R::Event>) {
        self.filter = Box::new(filter);
    }

    /// Returns a reference to the wrapped reactor.
    pub(crate) fn inner(&self) -> &R {
        &self.reactor
    }

    pub(crate) fn inner_mut(&mut self) -> &mut R {
        &mut self.reactor
    }
}

impl<R: Reactor> Reactor for FilterReactor<R> {
    type Event = R::Event;
    type Config = R::Config;
    type Error = R::Error;

    fn new(
        config: Self::Config,
        chainspec: Arc<Chainspec>,
        chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        network_identity: NetworkIdentity,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let (reactor, effects) = R::new(
            config,
            chainspec,
            chainspec_raw_bytes,
            network_identity,
            registry,
            event_queue,
            rng,
        )?;
        let filter = Box::new(Either::Right);
        Ok((Self { reactor, filter }, effects))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match (self.filter)(event) {
            Either::Left(effects) => effects,
            Either::Right(event) => self.reactor.dispatch_event(effect_builder, rng, event),
        }
    }

    fn activate_failpoint(&mut self, activation: &FailpointActivation) {
        self.reactor.activate_failpoint(activation);
    }
}

impl<R: Reactor + Finalize> Finalize for FilterReactor<R> {
    fn finalize(self) -> BoxFuture<'static, ()> {
        self.reactor.finalize()
    }
}

impl<R: Reactor + NetworkedReactor> NetworkedReactor for FilterReactor<R> {
    fn node_id(&self) -> NodeId {
        self.reactor.node_id()
    }
}

impl<R: Reactor + Debug> Debug for FilterReactor<R> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter
            .debug_struct("FilterReactor")
            .field("reactor", &self.reactor)
            .finish()
    }
}
