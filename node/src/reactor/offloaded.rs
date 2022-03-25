//! Offloaded components.
//!
//! TODO: Docs, Context

use datasize::DataSize;

use crate::{
    components::Component,
    effect::{EffectBuilder, Effects},
    types::NodeRng,
};

/// An offloading wrapper for a component.
///
/// Any component wrapped with `Offloaded<C>` will have its events processed off the main event
/// handling thread, i.e. all calls to `handle_event` will return immediately and process the event
/// in the background.
#[derive(Debug, DataSize)]
pub(crate) struct Offloaded<C> {
    /// The wrapped component.
    // Unfortunately, we have to skip `DataSize` pretty much entirely here.
    #[data_size(skip)]
    component: C,
}

impl<C> Offloaded<C> {
    /// Creates a new offloaded component.
    pub(crate) fn new(component: C) -> Self {
        Offloaded { component }
    }

    /// Deconstructs the offloaded component.
    pub(crate) fn into_inner(self) -> C {
        self.component
    }
}

impl<C, REv> Component<REv> for Offloaded<C>
where
    C: Component<REv>,
{
    type Event = <C as Component<REv>>::Event;

    type ConstructionError = <C as Component<REv>>::ConstructionError;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        self.component.handle_event(effect_builder, rng, event)
    }
}
