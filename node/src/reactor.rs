//! Reactor core.
//!
//! Any long running instance of the node application uses an event-dispatch pattern: Events are
//! generated and stored on an event queue, then processed one-by-one. This process happens inside
//! the reactor*, which also exclusively holds the state of the application besides pending events:
//!
//! 1. The reactor pops an event off the event queue (called a [`Scheduler`](type.Scheduler.html)).
//! 2. The event is dispatched by the reactor. Since the reactor holds mutable state, it can grant
//!    any component that processes an event mutable, exclusive access to its state.
//! 3. Once the (synchronous) event processing has completed, the component returns an effect.
//! 4. The reactor spawns a task that executes these effects and eventually schedules another event.
//! 5. meanwhile go to 1.
//!
//! # Reactors
//!
//! There is no single reactor, but rather a reactor for each application type, since it defines
//! which components are used and how they are wired up. The reactor defines the state by being a
//! `struct` of components, their initialization through the
//! [`Reactor::new()`](trait.Reactor.html#tymethod.new) and a method
//! [`Reactor::dispatch_event()`](trait.Reactor.html#tymethod.dispatch_event) to dispatch events to
//! components.
//!
//! With all these set up, a reactor can be executed using a [`Runner`](struct.Runner.html), either
//! in a step-wise manner using [`crank`](struct.Runner.html#method.crank) or indefinitely using
//! [`run`](struct.Runner.html#method.crank).

mod event_queue_metrics;
pub mod initializer;
pub mod joiner;
mod queue_kind;
pub mod validator;

use std::{
    collections::HashMap,
    fmt::{Debug, Display},
    mem,
};

use datasize::DataSize;
use futures::{future::BoxFuture, FutureExt};
use prometheus::{self, Histogram, HistogramOpts, IntCounter, Registry};
use quanta::IntoNanoseconds;
use tracing::{debug, debug_span, info, trace, warn};
use tracing_futures::Instrument;

use crate::{
    effect::{Effect, EffectBuilder, Effects},
    types::CryptoRngCore,
    utils::{self, WeightedRoundRobin},
};
use quanta::Clock;
pub use queue_kind::QueueKind;
use tokio::time::{Duration, Instant};

/// Threshold for when an event is considered slow.
const DISPATCH_EVENT_THRESHOLD: Duration = Duration::from_millis(1);

/// Event scheduler
///
/// The scheduler is a combination of multiple event queues that are polled in a specific order. It
/// is the central hook for any part of the program that schedules events directly.
///
/// Components rarely use this, but use a bound `EventQueueHandle` instead.
pub type Scheduler<Ev> = WeightedRoundRobin<Ev, QueueKind>;

/// Event queue handle
///
/// The event queue handle is how almost all parts of the application interact with the reactor
/// outside of the normal event loop. It gives different parts a chance to schedule messages that
/// stem from things like external IO.
#[derive(DataSize, Debug)]
pub struct EventQueueHandle<REv>(&'static Scheduler<REv>)
where
    REv: 'static;

// Implement `Clone` and `Copy` manually, as `derive` will make it depend on `R` and `Ev` otherwise.
impl<REv> Clone for EventQueueHandle<REv> {
    fn clone(&self) -> Self {
        EventQueueHandle(self.0)
    }
}
impl<REv> Copy for EventQueueHandle<REv> {}

impl<REv> EventQueueHandle<REv> {
    pub(crate) fn new(scheduler: &'static Scheduler<REv>) -> Self {
        EventQueueHandle(scheduler)
    }

    /// Schedule an event on a specific queue.
    #[inline]
    pub(crate) async fn schedule<Ev>(self, event: Ev, queue_kind: QueueKind)
    where
        REv: From<Ev>,
    {
        self.0.push(event.into(), queue_kind).await
    }

    /// Returns number of events in each of the scheduler's queues.
    pub(crate) fn event_queues_counts(&self) -> HashMap<QueueKind, usize> {
        self.0.event_queues_counts()
    }
}

/// Reactor core.
///
/// Any reactor should implement this trait and be executed by the `reactor::run` function.
pub trait Reactor: Sized {
    // Note: We've gone for the `Sized` bound here, since we return an instance in `new`. As an
    // alternative, `new` could return a boxed instance instead, removing this requirement.

    /// Event type associated with reactor.
    ///
    /// Defines what kind of event the reactor processes.
    type Event: Send + Debug + Display + 'static;

    /// A configuration for the reactor
    type Config;

    /// The error type returned by the reactor.
    type Error: Send + Sync + 'static;

    /// Dispatches an event on the reactor.
    ///
    /// This function is typically only called by the reactor itself to dispatch an event. It is
    /// safe to call regardless, but will cause the event to skip the queue and things like
    /// accounting.
    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut dyn CryptoRngCore,
        event: Self::Event,
    ) -> Effects<Self::Event>;

    /// Creates a new instance of the reactor.
    ///
    /// This method creates the full state, which consists of all components, and returns a reactor
    /// instance along with the effects that the components generated upon instantiation.
    ///
    /// If any instantiation fails, an error is returned.
    fn new(
        cfg: Self::Config,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut dyn CryptoRngCore,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error>;

    /// Indicates that the reactor has completed all its work and should no longer dispatch events.
    #[inline]
    fn is_stopped(&mut self) -> bool {
        false
    }

    /// Instructs the reactor to update performance metrics, if any.
    fn update_metrics(&mut self, _event_queue_handle: EventQueueHandle<Self::Event>) {}
}

/// A drop-like trait for `async` compatible drop-and-wait.
///
/// Shuts down a type by explicitly freeing resources, but allowing to wait on cleanup to complete.
pub trait Finalize: Sized {
    /// Runs cleanup code and waits for a shutdown to complete.
    ///
    /// This function must always be optional and a way to wait for all resources to be freed, not
    /// mandatory for cleanup!
    fn finalize(self) -> BoxFuture<'static, ()> {
        async move {}.boxed()
    }
}

/// A runner for a reactor.
///
/// The runner manages a reactors event queue and reactor itself and can run it either continuously
/// or in a step-by-step manner.
#[derive(Debug)]
pub struct Runner<R>
where
    R: Reactor,
{
    /// The scheduler used for the reactor.
    scheduler: &'static Scheduler<R::Event>,

    /// The reactor instance itself.
    reactor: R,

    /// Counter for events, to aid tracing.
    event_count: usize,

    /// Timestamp of last reactor metrics update.
    last_metrics: Instant,

    /// Metrics for the runner.
    metrics: RunnerMetrics,

    /// Check if we need to update reactor metrics every this many events.
    event_metrics_threshold: usize,

    /// Only update reactor metrics if at least this much time has passed.
    event_metrics_min_delay: Duration,

    /// An accurate, possible TSC-supporting clock.
    clock: Clock,
}

/// Metric data for the Runner
#[derive(Debug)]
struct RunnerMetrics {
    /// Total number of events processed.
    events: IntCounter,

    /// Histogram of how long it took to dispatch an event.
    event_dispatch_duration: Histogram,

    /// Handle to the metrics registry, in case we need to unregister.
    registry: Registry,
}

impl RunnerMetrics {
    /// Create and register new runner metrics.
    fn new(registry: &Registry) -> Result<Self, prometheus::Error> {
        let events = IntCounter::new("runner_events", "total event count")?;

        // Create an event dispatch histogram, putting extra emphasis on the area between 1-10 us.
        let event_dispatch_duration = Histogram::with_opts(
            HistogramOpts::new(
                "event_dispatch_duration",
                "duration of complete dispatch of a single event in nanoseconds",
            )
            .buckets(vec![
                100.0,
                500.0,
                1_000.0,
                5_000.0,
                10_000.0,
                20_000.0,
                50_000.0,
                100_000.0,
                200_000.0,
                300_000.0,
                400_000.0,
                500_000.0,
                600_000.0,
                700_000.0,
                800_000.0,
                900_000.0,
                1_000_000.0,
                2_000_000.0,
                5_000_000.0,
            ]),
        )?;

        registry.register(Box::new(events.clone()))?;
        registry.register(Box::new(event_dispatch_duration.clone()))?;

        Ok(RunnerMetrics {
            events,
            event_dispatch_duration,
            registry: registry.clone(),
        })
    }
}

impl Drop for RunnerMetrics {
    fn drop(&mut self) {
        self.registry
            .unregister(Box::new(self.events.clone()))
            .expect("did not expect deregistering events to fail");
        self.registry
            .unregister(Box::new(self.event_dispatch_duration.clone()))
            .expect("did not expect deregistering event_dispatch_duration to fail");
    }
}

impl<R> Runner<R>
where
    R: Reactor,
    R::Error: From<prometheus::Error>,
{
    /// Creates a new runner from a given configuration.
    ///
    /// Creates a metrics registry that is only going to be used in this runner.
    #[inline]
    pub async fn new(cfg: R::Config, rng: &mut dyn CryptoRngCore) -> Result<Self, R::Error> {
        // Instantiate a new registry for metrics for this reactor.
        let registry = Registry::new();
        Self::with_metrics(cfg, rng, &registry).await
    }

    /// Creates a new runner from a given configuration, using existing metrics.
    #[inline]
    pub async fn with_metrics(
        cfg: R::Config,
        rng: &mut dyn CryptoRngCore,
        registry: &Registry,
    ) -> Result<Self, R::Error> {
        let event_size = mem::size_of::<R::Event>();

        // Check if the event is of a reasonable size. This only emits a runtime warning at startup
        // right now, since storage size of events is not an issue per se, but copying might be
        // expensive if events get too large.
        if event_size > 16 * mem::size_of::<usize>() {
            warn!(%event_size, "large event size, consider reducing it or boxing");
        }

        let scheduler = utils::leak(Scheduler::new(QueueKind::weights()));

        let event_queue = EventQueueHandle::new(scheduler);
        let (reactor, initial_effects) = R::new(cfg, registry, event_queue, rng)?;

        // Run all effects from component instantiation.
        let span = debug_span!("process initial effects");
        process_effects(scheduler, initial_effects)
            .instrument(span)
            .await;

        info!("reactor main loop is ready");

        Ok(Runner {
            scheduler,
            reactor,
            event_count: 0,
            metrics: RunnerMetrics::new(registry)?,
            last_metrics: Instant::now(),
            event_metrics_min_delay: Duration::from_secs(30),
            event_metrics_threshold: 1000,
            clock: Clock::new(),
        })
    }

    /// Inject (schedule then process) effects created via a call to `create_effects` which is
    /// itself passed an instance of an `EffectBuilder`.
    #[cfg(test)]
    pub(crate) async fn process_injected_effects<F>(&mut self, create_effects: F)
    where
        F: FnOnce(EffectBuilder<R::Event>) -> Effects<R::Event>,
    {
        let event_queue = EventQueueHandle::new(self.scheduler);
        let effect_builder = EffectBuilder::new(event_queue);

        let effects = create_effects(effect_builder);

        let effect_span = debug_span!("process injected effects", ev = self.event_count);
        process_effects(self.scheduler, effects)
            .instrument(effect_span)
            .await;
    }

    /// Processes a single event on the event queue.
    #[inline]
    pub async fn crank(&mut self, rng: &mut dyn CryptoRngCore) {
        // Create another span for tracing the processing of one event.
        let crank_span = debug_span!("crank", ev = self.event_count);
        let _inner_enter = crank_span.enter();

        self.metrics.events.inc();

        let event_queue = EventQueueHandle::new(self.scheduler);
        let effect_builder = EffectBuilder::new(event_queue);

        // Update metrics like memory usage and event queue sizes.
        if self.event_count % self.event_metrics_threshold == 0 {
            let now = Instant::now();

            // We update metrics on the first very event as well to get a good baseline.
            if now.duration_since(self.last_metrics) >= self.event_metrics_min_delay
                || self.event_count == 0
            {
                self.reactor.update_metrics(event_queue);
                self.last_metrics = now;
            }
        }

        let (event, q) = self.scheduler.pop().await;

        // Create another span for tracing the processing of one event.
        let event_span = debug_span!("dispatch events", ev = self.event_count);
        let inner_enter = event_span.enter();

        // We log events twice, once in display and once in debug mode.
        debug!(%event, ?q);
        trace!(?event, ?q);

        // Dispatch the event, then execute the resulting effect.
        let start = self.clock.start();
        let effects = self.reactor.dispatch_event(effect_builder, rng, event);
        let end = self.clock.end();

        // Warn if processing took a long time, record to histogram.
        let delta = self.clock.delta(start, end);
        if delta > DISPATCH_EVENT_THRESHOLD {
            warn!(
                ns = delta.into_nanos(),
                "previous event took very long to dispatch"
            );
        }
        self.metrics
            .event_dispatch_duration
            .observe(delta.into_nanos() as f64);

        drop(inner_enter);

        // We create another span for the effects, but will keep the same ID.
        let effect_span = debug_span!("process effects", ev = self.event_count);

        process_effects(self.scheduler, effects)
            .instrument(effect_span)
            .await;

        self.event_count += 1;
    }

    /// Processes a single event if there is one, returns `None` otherwise.
    #[inline]
    pub async fn try_crank(&mut self, rng: &mut dyn CryptoRngCore) -> Option<()> {
        if self.scheduler.item_count() == 0 {
            None
        } else {
            self.crank(rng).await;
            Some(())
        }
    }

    /// Runs the reactor until `is_stopped()` returns true.
    #[inline]
    pub async fn run(&mut self, rng: &mut dyn CryptoRngCore) {
        while !self.reactor.is_stopped() {
            self.crank(rng).await;
        }
    }

    /// Returns a reference to the reactor.
    #[inline]
    pub fn reactor(&self) -> &R {
        &self.reactor
    }

    /// Returns a mutable reference to the reactor.
    #[inline]
    pub fn reactor_mut(&mut self) -> &mut R {
        &mut self.reactor
    }

    /// Deconstructs the runner to return the reactor.
    #[inline]
    pub fn into_inner(self) -> R {
        self.reactor
    }
}

/// Spawns tasks that will process the given effects.
#[inline]
async fn process_effects<Ev>(scheduler: &'static Scheduler<Ev>, effects: Effects<Ev>)
where
    Ev: Send + 'static,
{
    // TODO: Properly carry around priorities.
    let queue_kind = QueueKind::default();

    for effect in effects {
        tokio::spawn(async move {
            for event in effect.await {
                scheduler.push(event, queue_kind).await
            }
        });
    }
}

/// Converts a single effect into another by wrapping it.
#[inline]
fn wrap_effect<Ev, REv, F>(wrap: F, effect: Effect<Ev>) -> Effect<REv>
where
    F: Fn(Ev) -> REv + Send + 'static,
    Ev: Send + 'static,
    REv: Send + 'static,
{
    // TODO: The double-boxing here is very unfortunate =(.
    (async move {
        let events = effect.await;
        events.into_iter().map(wrap).collect()
    })
    .boxed()
}

/// Converts multiple effects into another by wrapping.
#[inline]
pub fn wrap_effects<Ev, REv, F>(wrap: F, effects: Effects<Ev>) -> Effects<REv>
where
    F: Fn(Ev) -> REv + Send + 'static + Clone,
    Ev: Send + 'static,
    REv: Send + 'static,
{
    effects
        .into_iter()
        .map(move |effect| wrap_effect(wrap.clone(), effect))
        .collect()
}
