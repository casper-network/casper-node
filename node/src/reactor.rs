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
pub mod participating;
mod queue_kind;

#[cfg(test)]
use std::sync::Arc;
use std::{
    any,
    collections::HashMap,
    env,
    fmt::{Debug, Display},
    fs::File,
    mem,
    str::FromStr,
    sync::atomic::Ordering,
};

use datasize::DataSize;
use futures::{future::BoxFuture, FutureExt};
use jemalloc_ctl::{epoch as jemalloc_epoch, stats::allocated as jemalloc_allocated};
use once_cell::sync::Lazy;
use prometheus::{self, Histogram, HistogramOpts, IntCounter, IntGauge, Registry};
use quanta::{Clock, IntoNanoseconds};
use serde::Serialize;
use signal_hook::consts::signal::{SIGINT, SIGQUIT, SIGTERM};
use tokio::time::{Duration, Instant};
use tracing::{debug, debug_span, error, info, instrument, trace, warn};
use tracing_futures::Instrument;

#[cfg(target_os = "linux")]
use utils::rlimit::{Limit, OpenFiles, ResourceLimit};

use crate::{
    effect::{announcements::ControlAnnouncement, Effect, EffectBuilder, Effects},
    types::{ExitCode, Timestamp},
    unregister_metric,
    utils::{self, WeightedRoundRobin},
    NodeRng, QUEUE_DUMP_REQUESTED, TERMINATION_REQUESTED,
};
#[cfg(test)]
use crate::{reactor::initializer::Reactor as InitializerReactor, types::Chainspec};
pub use queue_kind::QueueKind;

/// Optional upper threshold for total RAM allocated in mB before dumping queues to disk.
const MEM_DUMP_THRESHOLD_MB_ENV_VAR: &str = "CL_MEM_DUMP_THRESHOLD_MB";
static MEM_DUMP_THRESHOLD_MB: Lazy<Option<u64>> = Lazy::new(|| {
    env::var(MEM_DUMP_THRESHOLD_MB_ENV_VAR)
        .map(|threshold_str| {
            u64::from_str(&threshold_str).unwrap_or_else(|error| {
                panic!(
                    "can't parse env var {}={} as a u64: {}",
                    MEM_DUMP_THRESHOLD_MB_ENV_VAR, threshold_str, error
                )
            })
        })
        .ok()
});

/// Default threshold for when an event is considered slow.  Can be overridden by setting the env
/// var `CL_EVENT_MAX_MICROSECS=<MICROSECONDS>`.
const DEFAULT_DISPATCH_EVENT_THRESHOLD: Duration = Duration::from_secs(1);
const DISPATCH_EVENT_THRESHOLD_ENV_VAR: &str = "CL_EVENT_MAX_MICROSECS";

static DISPATCH_EVENT_THRESHOLD: Lazy<Duration> = Lazy::new(|| {
    env::var(DISPATCH_EVENT_THRESHOLD_ENV_VAR)
        .map(|threshold_str| {
            let threshold_microsecs = u64::from_str(&threshold_str).unwrap_or_else(|error| {
                panic!(
                    "can't parse env var {}={} as a u64: {}",
                    DISPATCH_EVENT_THRESHOLD_ENV_VAR, threshold_str, error
                )
            });
            Duration::from_micros(threshold_microsecs)
        })
        .unwrap_or_else(|_| DEFAULT_DISPATCH_EVENT_THRESHOLD)
});

#[cfg(target_os = "linux")]
/// The desired limit for open files.
const TARGET_OPEN_FILES_LIMIT: Limit = 64_000;

#[cfg(target_os = "linux")]
/// Adjusts the maximum number of open file handles upwards towards the hard limit.
fn adjust_open_files_limit() {
    // Ensure we have reasonable ulimits.
    match ResourceLimit::<OpenFiles>::get() {
        Err(err) => {
            warn!(%err, "could not retrieve open files limit");
        }

        Ok(current_limit) => {
            if current_limit.current() < TARGET_OPEN_FILES_LIMIT {
                let best_possible = if current_limit.max() < TARGET_OPEN_FILES_LIMIT {
                    warn!(
                        wanted = TARGET_OPEN_FILES_LIMIT,
                        hard_limit = current_limit.max(),
                        "settling for lower open files limit due to hard limit"
                    );
                    current_limit.max()
                } else {
                    TARGET_OPEN_FILES_LIMIT
                };

                let new_limit = ResourceLimit::<OpenFiles>::fixed(best_possible);
                if let Err(err) = new_limit.set() {
                    warn!(%err, current=current_limit.current(), target=best_possible, "did not succeed in raising open files limit")
                } else {
                    debug!(?new_limit, "successfully increased open files limit");
                }
            } else {
                debug!(
                    ?current_limit,
                    "not changing open files limit, already sufficient"
                );
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
/// File handle limit adjustment shim.
fn adjust_open_files_limit() {
    info!("not on linux, not adjusting open files limit");
}

/// The value returned by a reactor on completion of the `run()` loop.
#[derive(Clone, Copy, PartialEq, Eq, Debug, DataSize)]
pub enum ReactorExit {
    /// The process should continue running, moving to the next reactor.
    ProcessShouldContinue,
    /// The process should exit with the given exit code to allow the launcher to react
    /// accordingly.
    ProcessShouldExit(ExitCode),
}

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
    #[inline]
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
    type Event: ReactorEvent + Display;

    /// A configuration for the reactor
    type Config;

    /// The error type returned by the reactor.
    type Error: Send + 'static;

    /// Dispatches an event on the reactor.
    ///
    /// This function is typically only called by the reactor itself to dispatch an event. It is
    /// safe to call regardless, but will cause the event to skip the queue and things like
    /// accounting.
    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
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
        rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error>;

    /// If `Some`, indicates that the reactor has completed all its work and should no longer
    /// dispatch events.  The running process may stop or may keep running with a new reactor.
    fn maybe_exit(&self) -> Option<ReactorExit>;

    /// Instructs the reactor to update performance metrics, if any.
    fn update_metrics(&mut self, _event_queue_handle: EventQueueHandle<Self::Event>) {}
}

/// A reactor event type.
pub trait ReactorEvent: Send + Debug + From<ControlAnnouncement> + 'static {
    /// Returns the event as a control announcement, if possible.
    ///
    /// Returns a reference to a wrapped
    /// [`ControlAnnouncement`](`crate::effect::announcements::ControlAnnouncement`) if the event
    /// is indeed a control announcement variant.
    fn as_control(&self) -> Option<&ControlAnnouncement>;
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

/// Represents memory statistics in bytes.
struct AllocatedMem {
    /// Total allocated memory in bytes.
    allocated: u64,
    /// Total consumed memory in bytes.
    consumed: u64,
    /// Total system memory in bytes.
    total: u64,
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

    /// Last queue dump timestamp
    last_queue_dump: Option<Timestamp>,
}

/// Metric data for the Runner
#[derive(Debug)]
struct RunnerMetrics {
    /// Total number of events processed.
    events: IntCounter,
    /// Histogram of how long it took to dispatch an event.
    event_dispatch_duration: Histogram,
    /// Total allocated RAM in bytes, as reported by jemalloc.
    allocated_ram_bytes: IntGauge,
    /// Total consumed RAM in bytes, as reported by sys-info.
    consumed_ram_bytes: IntGauge,
    /// Total system RAM in bytes, as reported by sys-info.
    total_ram_bytes: IntGauge,
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

        let allocated_ram_bytes =
            IntGauge::new("allocated_ram_bytes", "total allocated ram in bytes")?;
        let consumed_ram_bytes =
            IntGauge::new("consumed_ram_bytes", "total consumed ram in bytes")?;
        let total_ram_bytes = IntGauge::new("total_ram_bytes", "total system ram in bytes")?;

        registry.register(Box::new(events.clone()))?;
        registry.register(Box::new(event_dispatch_duration.clone()))?;
        registry.register(Box::new(allocated_ram_bytes.clone()))?;
        registry.register(Box::new(consumed_ram_bytes.clone()))?;
        registry.register(Box::new(total_ram_bytes.clone()))?;

        Ok(RunnerMetrics {
            events,
            event_dispatch_duration,
            registry: registry.clone(),
            allocated_ram_bytes,
            consumed_ram_bytes,
            total_ram_bytes,
        })
    }
}

impl Drop for RunnerMetrics {
    fn drop(&mut self) {
        unregister_metric!(self.registry, self.events);
        unregister_metric!(self.registry, self.event_dispatch_duration);
        unregister_metric!(self.registry, self.allocated_ram_bytes);
        unregister_metric!(self.registry, self.consumed_ram_bytes);
        unregister_metric!(self.registry, self.total_ram_bytes);
    }
}

impl<R> Runner<R>
where
    R: Reactor,
    R::Event: Serialize,
    R::Error: From<prometheus::Error>,
{
    /// Creates a new runner from a given configuration.
    ///
    /// Creates a metrics registry that is only going to be used in this runner.
    #[inline]
    pub async fn new(cfg: R::Config, rng: &mut NodeRng) -> Result<Self, R::Error> {
        // Instantiate a new registry for metrics for this reactor.
        let registry = Registry::new();
        Self::with_metrics(cfg, rng, &registry).await
    }

    /// Creates a new runner from a given configuration, using existing metrics.
    #[inline]
    #[instrument("runner creation", level = "debug", skip(cfg, rng, registry))]
    pub async fn with_metrics(
        cfg: R::Config,
        rng: &mut NodeRng,
        registry: &Registry,
    ) -> Result<Self, R::Error> {
        adjust_open_files_limit();

        let event_size = mem::size_of::<R::Event>();

        // Check if the event is of a reasonable size. This only emits a runtime warning at startup
        // right now, since storage size of events is not an issue per se, but copying might be
        // expensive if events get too large.
        if event_size > 16 * mem::size_of::<usize>() {
            warn!(
                %event_size, type_name = ?any::type_name::<R::Event>(),
                "large event size, consider reducing it or boxing"
            );
        }

        let scheduler = utils::leak(Scheduler::new(QueueKind::weights()));

        let event_queue = EventQueueHandle::new(scheduler);
        let (reactor, initial_effects) = R::new(cfg, registry, event_queue, rng)?;

        // Run all effects from component instantiation.
        process_effects(scheduler, initial_effects)
            .instrument(debug_span!("process initial effects"))
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
            last_queue_dump: None,
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

        process_effects(self.scheduler, effects)
            .instrument(debug_span!(
                "process injected effects",
                ev = self.event_count
            ))
            .await;
    }

    /// Processes a single event on the event queue.
    ///
    /// Returns `false` if processing should stop.
    #[inline]
    #[instrument("crank", level = "debug", fields(ev = self.event_count), skip(self, rng))]
    pub async fn crank(&mut self, rng: &mut NodeRng) -> bool {
        self.metrics.events.inc();

        let event_queue = EventQueueHandle::new(self.scheduler);
        let effect_builder = EffectBuilder::new(event_queue);

        // Update metrics like memory usage and event queue sizes.
        if self.event_count % self.event_metrics_threshold == 0 {
            // We update metrics on the first very event as well to get a good baseline.
            if self.last_metrics.elapsed() >= self.event_metrics_min_delay {
                self.reactor.update_metrics(event_queue);

                // Use a fresh timestamp. This skews the metrics collection interval a little bit,
                // but ensures that if metrics collection time explodes, we are guaranteed a full
                // `event_metrics_min_delay` of event processing.
                self.last_metrics = Instant::now();
            }

            if let Some(AllocatedMem {
                allocated,
                consumed,
                total,
            }) = Self::get_allocated_memory()
            {
                debug!(%allocated, %total, "memory allocated");
                self.metrics.allocated_ram_bytes.set(allocated as i64);
                self.metrics.consumed_ram_bytes.set(consumed as i64);
                self.metrics.total_ram_bytes.set(total as i64);
                if let Some(threshold_mb) = *MEM_DUMP_THRESHOLD_MB {
                    let threshold_bytes = threshold_mb * 1024 * 1024;
                    if allocated >= threshold_bytes && self.last_queue_dump.is_none() {
                        info!(
                            %allocated,
                            %total,
                            %threshold_bytes,
                            "node has allocated enough memory to trigger queue dump"
                        );
                        self.dump_queues().await;
                    }
                }
            }
        }

        // Dump event queue if requested, stopping the world.
        if QUEUE_DUMP_REQUESTED.load(Ordering::SeqCst) {
            debug!("dumping event queue as requested");
            self.dump_queues().await;
            // Indicate we are done with the dump.
            QUEUE_DUMP_REQUESTED.store(false, Ordering::SeqCst);
        }

        let (event, q) = self.scheduler.pop().await;

        // Create another span for tracing the processing of one event.
        let event_span = debug_span!("dispatch events", ev = self.event_count);
        let (effects, keep_going) = event_span.in_scope(|| {
            // We log events twice, once in display and once in debug mode.
            let event_as_string = format!("{}", event);
            debug!(event=%event_as_string, ?q);
            trace!(?event, ?q);

            // Dispatch the event, then execute the resulting effect.
            let start = self.clock.start();

            let (effects, keep_going) = if let Some(ctrl_ann) = event.as_control() {
                // We've received a control event, which will _not_ be handled by the reactor.
                match ctrl_ann {
                    ControlAnnouncement::FatalError { file, line, msg } => {
                        error!(%file, %line, %msg, "fatal error via control announcement");
                        (Default::default(), false)
                    }
                }
            } else {
                (
                    self.reactor.dispatch_event(effect_builder, rng, event),
                    true,
                )
            };

            let end = self.clock.end();

            // Warn if processing took a long time, record to histogram.
            let delta = self.clock.delta(start, end);
            if delta > *DISPATCH_EVENT_THRESHOLD {
                warn!(
                    ns = delta.into_nanos(),
                    event = %event_as_string,
                    "event took very long to dispatch"
                );
            }
            self.metrics
                .event_dispatch_duration
                .observe(delta.into_nanos() as f64);

            (effects, keep_going)
        });

        process_effects(self.scheduler, effects)
            .instrument(debug_span!("process effects", ev = self.event_count))
            .await;

        self.event_count += 1;

        keep_going
    }

    /// Gets both the allocated and total memory from sys-info + jemalloc
    fn get_allocated_memory() -> Option<AllocatedMem> {
        let mem_info = match sys_info::mem_info() {
            Ok(mem_info) => mem_info,
            Err(error) => {
                warn!(%error, "unable to get mem_info using sys-info");
                return None;
            }
        };

        // mem_info gives us kB
        let total = mem_info.total * 1024;
        let consumed = total - (mem_info.free * 1024);

        // whereas jemalloc_ctl gives us the numbers in bytes
        match jemalloc_epoch::mib() {
            Ok(mib) => {
                // jemalloc_ctl requires you to advance the epoch to update its stats
                if let Err(advance_error) = mib.advance() {
                    warn!(%advance_error, "unable to advance jemalloc epoch");
                }
            }
            Err(error) => {
                warn!(%error, "unable to get epoch::mib from jemalloc");
                return None;
            }
        }
        let allocated = match jemalloc_allocated::mib() {
            Ok(allocated_mib) => match allocated_mib.read() {
                Ok(value) => value as u64,
                Err(error) => {
                    warn!(%error, "unable to read allocated mib using jemalloc");
                    return None;
                }
            },
            Err(error) => {
                warn!(%error, "unable to get allocated mib using jemalloc");
                return None;
            }
        };

        Some(AllocatedMem {
            allocated,
            consumed,
            total,
        })
    }

    /// Handles dumping queue contents to files in /tmp.
    async fn dump_queues(&mut self) {
        let timestamp = Timestamp::now();
        self.last_queue_dump = Some(timestamp);
        let output_fn = format!("/tmp/queue_dump-{}.json", timestamp);
        let mut serializer = serde_json::Serializer::pretty(match File::create(&output_fn) {
            Ok(file) => file,
            Err(error) => {
                warn!(%error, "could not create output file ({}) for queue snapshot", output_fn);
                return;
            }
        });

        if let Err(error) = self.scheduler.snapshot(&mut serializer).await {
            warn!(%error, "could not serialize snapshot to {}", output_fn);
            return;
        }

        let debug_dump_filename = format!("/tmp/queue_dump_debug-{}.txt", timestamp);
        let mut file = match File::create(&debug_dump_filename) {
            Ok(file) => file,
            Err(error) => {
                warn!(%error, "could not create debug output file ({}) for queue snapshot", debug_dump_filename);
                return;
            }
        };
        if let Err(error) = self.scheduler.debug_dump(&mut file).await {
            warn!(%error, "could not serialize debug snapshot to {}", debug_dump_filename);
            return;
        }
    }

    /// Processes a single event if there is one, returns `None` otherwise.
    #[inline]
    #[cfg(test)]
    pub async fn try_crank(&mut self, rng: &mut NodeRng) -> Option<bool> {
        if self.scheduler.item_count() == 0 {
            None
        } else {
            Some(self.crank(rng).await)
        }
    }

    /// Runs the reactor until `maybe_exit()` returns `Some` or we get interrupted by a termination
    /// signal.
    #[inline]
    pub async fn run(&mut self, rng: &mut NodeRng) -> ReactorExit {
        loop {
            match TERMINATION_REQUESTED.load(Ordering::SeqCst) as i32 {
                0 => {
                    if let Some(reactor_exit) = self.reactor.maybe_exit() {
                        // TODO: Workaround, until we actually use control announcements for
                        // exiting: Go over the entire remaining event queue and look for a control
                        // announcement. This approach is hacky, and should be replaced with
                        // `ControlAnnouncement` handling instead.

                        for event in self.scheduler.drain_queue(QueueKind::Control).await {
                            if let Some(ctrl_ann) = event.as_control() {
                                match ctrl_ann {
                                    ControlAnnouncement::FatalError { file, line, msg } => {
                                        warn!(%file, line=*line, %msg, "exiting due to fatal error scheduled before reactor completion");
                                        return ReactorExit::ProcessShouldExit(ExitCode::Abort);
                                    }
                                }
                            } else {
                                debug!(%event, "found non-control announcement while draining queue")
                            }
                        }

                        break reactor_exit;
                    }
                    if !self.crank(rng).await {
                        break ReactorExit::ProcessShouldExit(ExitCode::Abort);
                    }
                }
                SIGINT => break ReactorExit::ProcessShouldExit(ExitCode::SigInt),
                SIGQUIT => break ReactorExit::ProcessShouldExit(ExitCode::SigQuit),
                SIGTERM => break ReactorExit::ProcessShouldExit(ExitCode::SigTerm),
                _ => error!("should be unreachable - bug in signal handler"),
            }
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

    /// Shuts down a reactor, sealing and draining the entire queue before returning it.
    #[inline]
    pub async fn drain_into_inner(self) -> R {
        self.scheduler.seal();
        for event in self.scheduler.drain_queues().await {
            debug!(event=%event, "drained event");
        }
        self.reactor
    }
}

#[cfg(test)]
impl Runner<InitializerReactor> {
    pub(crate) async fn new_with_chainspec(
        cfg: <InitializerReactor as Reactor>::Config,
        chainspec: Arc<Chainspec>,
    ) -> Result<Self, <InitializerReactor as Reactor>::Error> {
        let registry = Registry::new();
        let scheduler = utils::leak(Scheduler::new(QueueKind::weights()));

        let event_queue = EventQueueHandle::new(scheduler);
        let (reactor, initial_effects) =
            InitializerReactor::new_with_chainspec(cfg, &registry, event_queue, chainspec)?;

        // Run all effects from component instantiation.
        let span = debug_span!("process initial effects");
        process_effects(scheduler, initial_effects)
            .instrument(span)
            .await;

        info!("reactor main loop is ready");

        let event_metrics_min_delay = Duration::from_secs(30);
        let now = Instant::now();
        Ok(Runner {
            scheduler,
            reactor,
            event_count: 0,
            metrics: RunnerMetrics::new(&registry)?,
            // Calculate the `last_metrics` timestamp to be exactly one delay in the past. This will
            // cause the runner to collect metrics at the first opportunity.
            last_metrics: now.checked_sub(event_metrics_min_delay).unwrap_or(now),
            event_metrics_min_delay,
            event_metrics_threshold: 1000,
            clock: Clock::new(),
            last_queue_dump: None,
        })
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
