//! Reactor core.
//!
//! Any long running instance of the node application uses an event-dispatch pattern: Events are
//! generated and stored on an event queue, then processed one-by-one. This process happens inside
//! the *reactor*, which also exclusively holds the state of the application besides pending events:
//!
//! 1. The reactor pops an event off of the event queue (called a `Scheduler`).
//! 2. The event is dispatched by the reactor. Since the reactor holds mutable state, it can grant
//!    any component that processes an event mutable, exclusive access to its state.
//! 3. Once the (synchronous) event processing has completed, the component returns an effect.
//! 4. The reactor spawns a task that executes these effects and eventually schedules another event.
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

use crate::util::Multiple;
use crate::{config, effect, util};
use async_trait::async_trait;
use std::{fmt, mem};
use tracing::{debug, info, warn};

pub use queue_kind::Queue;

/// Event scheduler
///
/// The scheduler is a combination of multiple event queues that are polled in a specific order. It
/// is the central hook for any part of the program that schedules events directly.
///
/// Components rarely use this, but use a bound `EventQueueHandle` instead.
pub type Scheduler<Ev> = util::round_robin::WeightedRoundRobin<Ev, Queue>;

/// Bound event queue handle
///
/// The event queue handle is how almost all parts of the application interact with the reactor
/// outside of the normal event loop. It gives different parts a chance to schedule messages that
/// stem from things like external IO.
///
/// Every event queue handle carries with it a reference to a wrapper function that allows a
/// particular event to be wrapped again into a reactor event. Every component requiring access is
/// passed its own instance of the queue handle.
#[derive(Debug)]
pub struct EventQueueHandle<Ev: 'static, W> {
    /// The scheduler events will be scheduled on.
    scheduler: &'static Scheduler<Ev>,
    /// A wrapper function translating from component event (input of `W`) to reactor event `Ev`.
    wrapper: W,
}

// Clone needs to be implemented manually, since `Ev` prevents derivation.
impl<Ev, W> Clone for EventQueueHandle<Ev, W>
where
    W: Clone,
{
    fn clone(&self) -> Self {
        EventQueueHandle {
            scheduler: self.scheduler,
            wrapper: self.wrapper.clone(),
        }
    }
}

impl<Ev, W> EventQueueHandle<Ev, W>
where
    Ev: Send + 'static,
{
    /// Create a new event queue handle with an associated wrapper function.
    fn bind(scheduler: &'static Scheduler<Ev>, wrapper: W) -> Self {
        EventQueueHandle { scheduler, wrapper }
    }

    /// Schedule an event on a specific queue.
    #[inline]
    pub async fn schedule<SubEv>(self, event: SubEv, queue_kind: Queue)
    where
        W: Fn(SubEv) -> Ev + 'static,
    {
        self.scheduler.push((self.wrapper)(event), queue_kind).await
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
    fn dispatch_event(&mut self, event: Self::Event) -> Multiple<effect::Effect<Self::Event>>;

    /// Create a new instance of the reactor.
    ///
    /// This method creates the full state, which consists of all components, and returns a reactor
    /// instances along with the effects the components generated upon instantiation.
    ///
    /// If any instantiation fails, an error is returned.
    fn new(
        cfg: &config::Config,
        scheduler: &'static Scheduler<Self::Event>,
    ) -> anyhow::Result<(Self, Multiple<effect::Effect<Self::Event>>)>;
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
    if event_size > 4 * mem::size_of::<usize>() {
        warn!(
            "event size is {} bytes, consider reducing it or boxing",
            event_size
        );
    }

    let scheduler = Scheduler::<R::Event>::new(Queue::weights());

    // Create a new event queue for this reactor run.
    let scheduler = util::leak(scheduler);

    let (mut reactor, initial_effects) = R::new(&cfg, scheduler)?;

    // Run all effects from component instantiation.
    process_effects(scheduler, initial_effects).await;

    info!("entering reactor main loop");
    loop {
        let (event, q) = scheduler.pop().await;
        debug!(?event, ?q, "event");

        // Dispatch the event, then execute the resulting effect.
        let effects = reactor.dispatch_event(event);
        process_effects(scheduler, effects).await;
    }
}

/// Process effects.
///
/// Spawns tasks that will process the given effects.
#[inline]
async fn process_effects<Ev>(
    scheduler: &'static Scheduler<Ev>,
    effects: Multiple<effect::Effect<Ev>>,
) where
    Ev: Send + 'static,
{
    // TODO: Properly carry around priorities.
    let queue = Queue::default();

    for effect in effects {
        tokio::spawn(async move {
            for event in effect.await {
                scheduler.push(event, queue).await
            }
        });
    }
}
