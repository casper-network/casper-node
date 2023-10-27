use std::{
    fmt::{self, Debug, Formatter},
    sync::Arc,
};

use futures::future::BoxFuture;
use prometheus::Registry;

use super::network::NetworkedReactor;
use crate::{
    components::network::Identity as NetworkIdentity,
    effect::{EffectBuilder, Effects},
    reactor::{EventQueueHandle, Finalize, Reactor},
    types::{Chainspec, ChainspecRawBytes, NodeId},
    NodeRng,
};

type ConditionChecker<R> = Box<dyn Fn(&<R as Reactor>::Event) -> bool + Send>;

/// A reactor wrapping an inner reactor, and which has an optional hook into
/// `Reactor::dispatch_event()`.
///
/// While the hook is not `None`, it's called on every call to `dispatch_event()`, taking a
/// reference to the current `Event`, and setting a boolean result to true when the condition has
/// been met.
///
/// Once the condition is met, the hook is reset to `None`.
pub(crate) struct ConditionCheckReactor<R: Reactor> {
    reactor: R,
    condition_checker: Option<ConditionChecker<R>>,
    condition_result: bool,
}

impl<R: Reactor> ConditionCheckReactor<R> {
    /// Sets the condition checker hook.
    pub(crate) fn set_condition_checker(&mut self, condition_checker: ConditionChecker<R>) {
        self.condition_checker = Some(condition_checker);
        self.condition_result = false;
    }

    /// Returns the result of the last execution of the condition checker hook.
    pub(crate) fn condition_result(&self) -> bool {
        self.condition_result
    }

    /// Returns a reference to the wrapped reactor.
    pub(crate) fn inner(&self) -> &R {
        &self.reactor
    }

    /// Returns a mutable reference to the wrapped reactor.
    pub(crate) fn inner_mut(&mut self) -> &mut R {
        &mut self.reactor
    }
}

impl<R: Reactor> Reactor for ConditionCheckReactor<R> {
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
        Ok((
            Self {
                reactor,
                condition_checker: None,
                condition_result: false,
            },
            effects,
        ))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        self.condition_result = self
            .condition_checker
            .as_ref()
            .map(|condition_checker| condition_checker(&event))
            .unwrap_or_default();
        if self.condition_result {
            self.condition_checker = None;
        }
        self.reactor.dispatch_event(effect_builder, rng, event)
    }
}

impl<R: Reactor + Finalize> Finalize for ConditionCheckReactor<R> {
    fn finalize(self) -> BoxFuture<'static, ()> {
        self.reactor.finalize()
    }
}

impl<R: Reactor + NetworkedReactor> NetworkedReactor for ConditionCheckReactor<R> {
    fn node_id(&self) -> NodeId {
        self.reactor.node_id()
    }
}

impl<R: Reactor + Debug> Debug for ConditionCheckReactor<R> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter
            .debug_struct("ConditionCheckReactor")
            .field("reactor", &self.reactor)
            .field("condition_check_result", &self.condition_result)
            .finish()
    }
}
