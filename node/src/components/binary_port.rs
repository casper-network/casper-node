//! The Binary Port
mod error;
mod event;
mod metrics;
#[cfg(test)]
mod tests;

use datasize::DataSize;
use futures::{future::BoxFuture, FutureExt};
use prometheus::Registry;
use tracing::{info, warn};

use crate::{
    effect::{EffectBuilder, Effects},
    reactor::{main_reactor::MainEvent, Finalize},
    types::NodeRng,
    utils::ListeningError,
};

use self::metrics::Metrics;

use super::{Component, ComponentState, InitializedComponent, PortBoundComponent};
pub(crate) use event::Event;

const COMPONENT_NAME: &str = "binary_port";

#[derive(Debug, DataSize)]
pub(crate) struct BinaryPort {
    #[data_size(skip)]
    metrics: Metrics,
    state: ComponentState,
}

impl BinaryPort {
    pub(crate) fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        Ok(Self {
            state: ComponentState::Uninitialized,
            metrics: Metrics::new(registry)?,
        })
    }
}

impl<REv> Component<REv> for BinaryPort
where
    REv: From<Event> + Send,
{
    type Event = Event;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        _rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match &self.state {
            ComponentState::Uninitialized => {
                warn!(
                    ?event,
                    name = <Self as Component<MainEvent>>::name(self),
                    "should not handle this event when component is uninitialized"
                );
                Effects::new()
            }
            ComponentState::Initializing => match event {
                Event::Initialize => {
                    let (effects, state) = self.bind(true, effect_builder); // TODO[RC]: `true` should become `config.enabled`
                    <Self as InitializedComponent<MainEvent>>::set_state(self, state);
                    effects
                }
            },
            ComponentState::Initialized => todo!(),
            ComponentState::Fatal(_) => todo!(),
        }
    }

    fn name(&self) -> &str {
        COMPONENT_NAME
    }
}

impl<REv> InitializedComponent<REv> for BinaryPort
where
    REv: From<Event> + Send,
{
    fn state(&self) -> &ComponentState {
        &self.state
    }

    fn set_state(&mut self, new_state: ComponentState) {
        info!(
            ?new_state,
            name = <Self as Component<MainEvent>>::name(self),
            "component state changed"
        );

        self.state = new_state;
    }
}

impl<REv> PortBoundComponent<REv> for BinaryPort
where
    REv: From<Event> + Send,
{
    type Error = ListeningError;
    type ComponentEvent = Event;

    fn listen(
        &mut self,
        _effect_builder: EffectBuilder<REv>,
    ) -> Result<Effects<Self::ComponentEvent>, Self::Error> {
        // TODO: Start juliet server here

        Ok(Effects::new())
    }
}

impl Finalize for BinaryPort {
    fn finalize(self) -> BoxFuture<'static, ()> {
        // TODO: Shutdown juliet server here
        async move {}.boxed()
    }
}
