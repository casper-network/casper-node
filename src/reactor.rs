//! Reactor core.
//!
//! Any long running instance of the node application uses an event-dispatch pattern: Events are
//! generated and stored on an event queue, then processed one-by-one. This process happens inside
//! the *reactor*, which also exclusively holds the state of the application:
//!
//! 1. The reactor pops an event off of the queue.
//! 2. The event is dispatched by the reactor. Since the reactor holds mutable state, it can grant
//!    any component that processes an event mutable, exclusive access to its state.
//! 3. Once the (synchronous) event processing has completed, the component returns an effect.
//! 4. The reactor spawns a task that executes these effects and eventually puts an event onto the
//!    event queue.
//! 5. meanwhile go to 1.
//!
//! # Reactors
//!
//! There no single reactor, but a reactor for each application type, since it defines which
//! components are used and how they are wired up. The reactor defines the state by being a `struct`
//! of components, their initialization through the `Reactor::new` and a function to `dispatch`
//! events to components.
//!
//! With all these set up, a reactor can be `launch`ed, causing it to run indefinitely, processing
//! events.

pub mod non_validator;
mod queue_kind;
pub mod validator;

use crate::{config, effect, util};
use async_trait::async_trait;
use std::{fmt, mem};
use tracing::{debug, info, warn};

pub use queue_kind::Queue;

/// Event queue handle
///
/// The event queue handle is how almost all parts of the application interact with the reactor
/// outside of the normal event loop. It gives different parts a chance to schedule messages that
/// stem from things like external IO.
///
/// It is also possible to schedule new events by directly processing effects. This allows re-use of
/// the existing code for handling particular effects, as adding events directly should be a matter
/// of last resort.
#[derive(Debug)]
pub struct EventQueueHandle<Ev: 'static>(&'static util::round_robin::WeightedRoundRobin<Ev, Queue>);

// Copy and Clone need to be implemented manually, since `Ev` prevents derivation.
impl<Ev> Copy for EventQueueHandle<Ev> {}
impl<Ev> Clone for EventQueueHandle<Ev> {
    fn clone(&self) -> Self {
        EventQueueHandle(self.0)
    }
}

impl<Ev> EventQueueHandle<Ev>
where
    Ev: Send + 'static,
{
    /// Create a new event queue handle.
    fn new(round_robin: &'static util::round_robin::WeightedRoundRobin<Ev, Queue>) -> Self {
        EventQueueHandle(round_robin)
    }

    /// Return the next event in the queue
    ///
    /// Awaits until there is an event, then returns it.
    #[inline]
    async fn next_event(self) -> (Ev, Queue) {
        self.0.pop().await
    }

    /// Process an effect.
    ///
    /// This function simply spawns another task that will take of the effect
    /// processing.
    #[inline]
    pub fn process_effect(self, effect: effect::Effect<Ev>) {
        let eq = self;
        // TODO: Properly carry around priorities.
        let queue = Default::default();
        tokio::spawn(async move {
            for event in effect.await {
                eq.schedule(event, queue).await;
            }
        });
    }

    /// Schedule an event in the given queue.
    #[inline]
    pub async fn schedule(self, event: Ev, queue_kind: Queue) {
        self.0.push(event, queue_kind).await
    }
}

/// Reactor core.
///
/// Any reactor implements should implement this trait and be launched by the `launch` function.
#[async_trait]
pub trait Reactor: Sized {
    // Note: We've gone for the `Sized` bound here, since we return an instance in `new`. As an
    // alternative, `new` could return a boxed instance instead, removing this requirement.

    /// Event type associated with reactor.
    ///
    /// Defines what kind of event the reactor processes.
    type Event: Send + fmt::Debug + 'static;

    /// Dispatch an event on the reactor.
    ///
    /// This function is typically only called by the reactor itself to dispatch an event. It is
    /// safe to call regardless, but will cause the event to skip the queue and things like
    /// accounting.
    fn dispatch_event(&mut self, event: Self::Event) -> effect::Effect<Self::Event>;

    /// Create a new instance of the reactor.
    ///
    /// This method creates the full state, which consists of all components, and returns a reactor
    /// instances along with the effects the components generated upon instantiation.
    ///
    /// If any instantiation fails, an error is returned.
    fn new(
        cfg: &config::Config,
        eq: EventQueueHandle<Self::Event>,
    ) -> anyhow::Result<(Self, Vec<effect::Effect<Self::Event>>)>;
}

/// Run a reactor.
///
/// Start the reactor and associated background tasks, then enter main the event processing loop.
///
/// `launch` will leak memory on start for global structures each time it is called.
///
/// Errors are returned only if component initialization fails.
#[inline]
pub async fn launch<R: Reactor>(cfg: config::Config) -> anyhow::Result<()> {
    let event_size = mem::size_of::<R::Event>();
    // Check if the event is of a reasonable size. This only emits a runtime warning at startup
    // right now, since storage size of events is not an issue per se, but copying might be
    // expensive if events get too large.
    if event_size <= 4 * mem::size_of::<usize>() {
        warn!(
            "event size is {} bytes, consider reducing it or boxing",
            event_size
        );
    }

    let scheduler = util::round_robin::WeightedRoundRobin::<R::Event, Queue>::new(Queue::weights());

    // Create a new event queue for this reactor run.
    let eq = EventQueueHandle::new(util::leak(scheduler));

    let (mut reactor, initial_effects) = R::new(&cfg, eq)?;

    // Run all effects from component instantiation.
    for effect in initial_effects {
        eq.process_effect(effect);
    }

    info!("entering reactor main loop");
    loop {
        let (event, q) = eq.next_event().await;
        debug!(?event, ?q, "event");

        // Dispatch the event, then execute the resulting effect.
        let effect = reactor.dispatch_event(event);
        eq.process_effect(effect);
    }
}
