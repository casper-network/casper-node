use std::fmt::{self, Debug, Formatter};

use futures::{future::BoxFuture, FutureExt};
use prometheus::Registry;

use super::network::NetworkedReactor;
use crate::{
    effect::{EffectBuilder, Effects},
    reactor::{EventQueueHandle, Finalize, FutureResult, Reactor},
    testing::TestRng,
};

/// A reactor wrapping an inner reactor, and which has an optional hook into
/// `Reactor::dispatch_event()`.
///
/// While the hook is not `None`, it's called on every call to `dispatch_event()`, taking a
/// reference to the current `Event`, and setting a boolean result to true when the condition has
/// been met.
///
/// Once the condition is met, the hook is reset to `None`.
pub struct ConditionCheckReactor<R: Reactor<TestRng>> {
    reactor: R,
    condition_checker: Option<Box<dyn Fn(&R::Event) -> bool + Send>>,
    condition_result: bool,
}

impl<R: Reactor<TestRng>> ConditionCheckReactor<R> {
    /// Sets the condition checker hook.
    pub fn set_condition_checker(
        &mut self,
        condition_checker: Box<dyn Fn(&R::Event) -> bool + Send>,
    ) {
        self.condition_checker = Some(condition_checker);
    }

    /// Returns the result of the last execution of the condition checker hook.
    pub fn condition_result(&self) -> bool {
        self.condition_result
    }

    /// Returns a reference to the wrapped reactor.
    pub fn inner(&self) -> &R {
        &self.reactor
    }

    /// Returns a mutable reference to the wrapped reactor.
    pub fn inner_mut(&mut self) -> &mut R {
        &mut self.reactor
    }
}

impl<R: Reactor<TestRng> + 'static> Reactor<TestRng> for ConditionCheckReactor<R>
where
    R::Config: Send,
{
    type Event = R::Event;
    type Config = R::Config;
    type Error = R::Error;

    fn new(
        config: Self::Config,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut TestRng,
    ) -> FutureResult<(Self, Effects<Self::Event>), Self::Error> {
        let reactor_result_future = R::new(config, registry, event_queue, rng);
        async move {
            let (reactor, effects) = reactor_result_future.await?;
            Ok((
                Self {
                    reactor,
                    condition_checker: None,
                    condition_result: false,
                },
                effects,
            ))
        }
        .boxed()
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut TestRng,
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

impl<R: Reactor<TestRng> + Finalize> Finalize for ConditionCheckReactor<R> {
    fn finalize(self) -> BoxFuture<'static, ()> {
        self.reactor.finalize()
    }
}

impl<R: Reactor<TestRng> + NetworkedReactor> NetworkedReactor for ConditionCheckReactor<R> {
    type NodeId = R::NodeId;

    fn node_id(&self) -> Self::NodeId {
        self.reactor.node_id()
    }
}

impl<R: Reactor<TestRng> + Debug> Debug for ConditionCheckReactor<R> {
    fn fmt(&self, formatter: &mut Formatter) -> fmt::Result {
        formatter
            .debug_struct("ConditionCheckReactor")
            .field("reactor", &self.reactor)
            .field("condition_check_result", &self.condition_result)
            .finish()
    }
}
