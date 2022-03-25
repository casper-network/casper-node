//! Offloaded components.
//!
//! TODO: Docs, Context

use std::{
    marker::PhantomData,
    sync::mpsc,
    thread::{self, JoinHandle},
};

use datasize::DataSize;
use tracing::{debug, warn};

use crate::{
    components::Component,
    effect::{announcements::ControlAnnouncement, EffectBuilder, EffectExt, Effects},
    fatal,
    types::NodeRng,
    utils::child_rng::ChildRng,
};

/// An offloading wrapper for a component.
///
/// Any component wrapped with `Offloaded<C>` will have its events processed off the main event
/// handling thread, i.e. all calls to `handle_event` will return immediately and process the event
/// in the background.
#[derive(Debug)]
pub(crate) struct Offloaded<REv, C>
where
    C: Component<REv>,
{
    /// The wrapped component's runner thread join handle.
    component_runner: JoinHandle<C>,
    /// Channel used to send events to the component.
    event_sender: mpsc::Sender<<C as Component<REv>>::Event>,
    /// Random number generator channel sender.
    ///
    /// Used once to send down a new RNG once available.
    rng_sender: Option<mpsc::Sender<NodeRng>>,
    _phantom: PhantomData<REv>,
}

// Unfortunately, we have to skip `DataSize` pretty much entirely here.
impl<REv, C> DataSize for Offloaded<REv, C>
where
    C: Component<REv>,
{
    const IS_DYNAMIC: bool = false;

    const STATIC_HEAP_SIZE: usize = 0;

    fn estimate_heap_size(&self) -> usize {
        0
    }
}

/// Runs a component, processing events received on the `event_receiver` channel.
///
/// At startup, the runner first waits to acquire an RNG through the `rng_receiver`.
///
/// Exits once the events channel is closed, or if not RNG could be obtained.
fn component_runner<REv, C>(
    effect_builder: EffectBuilder<REv>,
    event_receiver: mpsc::Receiver<<C as Component<REv>>::Event>,
    mut component: C,
    rng_receiver: mpsc::Receiver<NodeRng>,
) -> C
where
    REv: From<ControlAnnouncement> + Send,
    C: Component<REv>,
    REv: 'static,
{
    let mut rng = match rng_receiver.recv() {
        Ok(rng) => rng,
        Err(_) => {
            warn!("did not receive rng in component runner, no events processed");
            return component;
        }
    };

    // Close the channel, just in case.
    drop(rng_receiver);

    while let Ok(event) = event_receiver.recv() {
        let effects = component.handle_event(effect_builder, &mut rng, event);

        // todo!("handle effects");
    }

    debug!("shutting down component runner after channel was closed");

    component
}

impl<REv, C> Offloaded<REv, C>
where
    C: Component<REv> + Send + 'static,
    <C as Component<REv>>::Event: Send + 'static,
    REv: From<ControlAnnouncement> + Send + 'static,
{
    /// Creates a new offloaded component.
    pub(crate) fn new(effect_builder: EffectBuilder<REv>, component: C) -> Self {
        let (event_sender, event_receiver) = mpsc::channel();
        let (rng_sender, rng_receiver) = mpsc::channel();

        // Start background thread for running the component.
        let component_runner = thread::spawn(move || {
            component_runner(effect_builder, event_receiver, component, rng_receiver)
        });

        Offloaded {
            component_runner,
            event_sender,
            rng_sender: Some(rng_sender),
            _phantom: PhantomData,
        }
    }

    /// Deconstructs the offloaded component.
    ///
    /// # Panics
    ///
    /// Panics if the wrapped component has panicked.
    pub(crate) fn into_inner(self) -> C {
        let Offloaded {
            component_runner,
            event_sender,
            ..
        } = self;

        // Dropping the sending channel causes the background thad to exit.
        drop(event_sender);

        // Note: We usually try to avoid panicks of any kind, but in this case it could have only
        // been caused by another panic. TODO: Remove unwrap.
        component_runner.join().expect("component thread panicked")
    }
}

impl<C, REv> Component<REv> for Offloaded<REv, C>
where
    C: Component<REv>,
    REv: From<ControlAnnouncement> + Send,
{
    type Event = <C as Component<REv>>::Event;

    type ConstructionError = <C as Component<REv>>::ConstructionError;

    fn handle_event(
        &mut self,
        effect_builder: EffectBuilder<REv>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        // Initialize the RNG if not initialized.
        if let Some(rng_sender) = self.rng_sender.take() {
            if let Err(_) = rng_sender.send(rng.new_child()) {
                return fatal!(effect_builder, "could not send RNG down to component").ignore();
            }
        }

        match self.event_sender.send(event) {
            Ok(()) => Effects::new(),
            Err(err) => fatal!(
                effect_builder,
                "failed to send event to offloaded component: {}",
                err
            )
            .ignore(),
        }
    }
}
