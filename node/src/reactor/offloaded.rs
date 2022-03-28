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
    fatal, reactor,
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
/// At startup, the runner first waits to acquire an RNG through the `rng_receiver`. Any effects
/// from processing the events will be passed on through `effects_sender`.
///
/// Exits once the events channel is closed, no RNG could be obtained at the start or sending
/// passing on effects failed.
fn component_runner<REv, C, W>(
    effect_builder: EffectBuilder<REv>,
    event_receiver: mpsc::Receiver<<C as Component<REv>>::Event>,
    mut component: C,
    rng_receiver: mpsc::Receiver<NodeRng>,
    effects_sender: tokio::sync::mpsc::UnboundedSender<Effects<REv>>,
    event_wrap: W,
) -> C
where
    REv: From<ControlAnnouncement> + Send + 'static,
    C: Component<REv>,
    <C as Component<REv>>::Event: Send + 'static,
    W: Fn(<C as Component<REv>>::Event) -> REv + Send + Clone + 'static,
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
        let effects = reactor::wrap_effects(
            event_wrap.clone(),
            component.handle_event(effect_builder, &mut rng, event),
        );

        if effects_sender.send(effects).is_err() {
            todo!()
        }
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
    // Result<(SmallNetwork<REv, P>, Effects<Event<P>>)>

    /// Creates a new offloaded component.
    pub(crate) fn new<W>(
        event_wrap: W,
        effect_builder: EffectBuilder<REv>,
        component: C,
    ) -> (Self, Effects<<C as Component<REv>>::Event>)
    where
        W: Fn(<C as Component<REv>>::Event) -> REv + Send + Clone + 'static,
    {
        let (event_sender, event_receiver) = mpsc::channel();
        let (rng_sender, rng_receiver) = mpsc::channel();
        let (effects_sender, mut effects_receiver) = tokio::sync::mpsc::unbounded_channel();

        // Start background thread for running the component.
        let component_runner = thread::spawn(move || {
            component_runner(
                effect_builder,
                event_receiver,
                component,
                rng_receiver,
                effects_sender,
                event_wrap,
            )
        });

        // Note: The `effects_receiving_task` will be the only effect returned here, and an outer
        // `wrap_effects` is entirely unnecessary, as we always call ignore in it. This function
        // still returns `Effects<<C as Component<REv>>::Event>` to keep in line with the typical
        // signature of other components.
        let effects_receiving_task = async move {
            while let Some(effects) = effects_receiver.recv().await {
                let scheduler = effect_builder.into_inner().raw_scheduler();
                reactor::process_effects(
                    None, // TOOD: Preserve ancestors.
                    scheduler, effects,
                )
                .await;
            }
        }
        .ignore();

        (
            Offloaded {
                component_runner,
                event_sender,
                rng_sender: Some(rng_sender),
                _phantom: PhantomData,
            },
            effects_receiving_task,
        )
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
            if rng_sender.send(rng.new_child()).is_err() {
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
