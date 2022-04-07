//! Reactor core.
//!
//! Any long running instance of the node application uses an event-dispatch pattern: Events are
//! generated and stored on an event queue, then processed one-by-one. This process happens inside
//! the reactor, which also exclusively holds the state of the application besides pending events:
//!
//! 1. The reactor pops a reactor event off the event queue (called a
//!    [`Scheduler`](type.Scheduler.html)).
//! 2. The event is dispatched by the reactor via [`Reactor::dispatch_event`]. Since the reactor
//!    holds mutable state, it can grant any component that processes an event mutable, exclusive
//!    access to its state.
//! 3. Once the [(synchronous)](`crate::components::Component::handle_event`) event processing has
//!    completed, the component returns an [`effect`](crate::effect).
//! 4. The reactor spawns a task that executes these effects and possibly schedules more events.
//! 5. go to 1.
//!
//! For descriptions of events and instructions on how to create effects, see the
//! [`effect`](super::effect) module.
//!
//! # Reactors
//!
//! There is no single reactor, but rather a reactor for each application type, since it defines
//! which components are used and how they are wired up. The reactor defines the state by being a
//! `struct` of components, their initialization through [`Reactor::new`] and event dispatching to
//! components via [`Reactor::dispatch_event`].
//!
//! With all these set up, a reactor can be executed using a [`Runner`], either in a step-wise
//! manner using [`Runner::crank`] or indefinitely using [`Runner::run`].

mod event_queue_metrics;
pub(crate) mod initializer;
pub(crate) mod joiner;
pub(crate) mod participating;
mod queue_kind;

#[cfg(test)]
use std::sync::Arc;
use std::{
    any,
    collections::HashMap,
    env,
    fmt::{Debug, Display},
    io::Write,
    mem,
    num::NonZeroU64,
    str::FromStr,
    sync::atomic::Ordering,
};

use datasize::DataSize;
use erased_serde::Serialize as ErasedSerialize;
use futures::{future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use prometheus::{self, Histogram, HistogramOpts, IntCounter, IntGauge, Registry};
use quanta::{Clock, IntoNanoseconds};
use serde::Serialize;
use signal_hook::consts::signal::{SIGINT, SIGQUIT, SIGTERM};
use tokio::time::{Duration, Instant};
use tracing::{debug, debug_span, error, info, instrument, trace, warn, Span};
use tracing_futures::Instrument;

use casper_types::EraId;
use utils::rlimit::{Limit, OpenFiles, ResourceLimit};

use crate::{
    components::fetcher,
    effect::{
        announcements::{BlocklistAnnouncement, ControlAnnouncement, QueueDumpFormat},
        Effect, EffectBuilder, EffectExt, Effects,
    },
    types::{ExitCode, Item, NodeId},
    unregister_metric,
    utils::{self, SharedFlag, WeightedRoundRobin},
    NodeRng, TERMINATION_REQUESTED,
};
#[cfg(test)]
use crate::{
    reactor::initializer::Reactor as InitializerReactor,
    types::{Chainspec, ChainspecRawBytes},
};
pub(crate) use queue_kind::QueueKind;
use stats_alloc::{Stats, INSTRUMENTED_SYSTEM};

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

/// The desired limit for open files.
const TARGET_OPEN_FILES_LIMIT: Limit = 64_000;

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

/// The value returned by a reactor on completion of the `run()` loop.
#[derive(Clone, Copy, PartialEq, Eq, Debug, DataSize)]
pub(crate) enum ReactorExit {
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
///
/// Schedule tuples contain an optional ancestor ID and the actual event. The ancestor ID indicates
/// which potential previous event resulted in the event being created.
pub(crate) type Scheduler<Ev> = WeightedRoundRobin<(Option<NonZeroU64>, Ev), QueueKind>;

/// Event queue handle
///
/// The event queue handle is how almost all parts of the application interact with the reactor
/// outside of the normal event loop. It gives different parts a chance to schedule messages that
/// stem from things like external IO.
#[derive(DataSize, Debug)]
pub(crate) struct EventQueueHandle<REv>
where
    REv: 'static,
{
    /// A reference to the scheduler of the event queue.
    scheduler: &'static Scheduler<REv>,
    /// Flag indicating whether or not the reactor processing this event queue is shutting down.
    is_shutting_down: SharedFlag,
}

// Implement `Clone` and `Copy` manually, as `derive` will make it depend on `R` and `Ev` otherwise.
impl<REv> Clone for EventQueueHandle<REv> {
    fn clone(&self) -> Self {
        EventQueueHandle {
            scheduler: self.scheduler,
            is_shutting_down: self.is_shutting_down,
        }
    }
}
impl<REv> Copy for EventQueueHandle<REv> {}

impl<REv> EventQueueHandle<REv> {
    /// Creates a new event queue handle.
    pub(crate) fn new(scheduler: &'static Scheduler<REv>, is_shutting_down: SharedFlag) -> Self {
        EventQueueHandle {
            scheduler,
            is_shutting_down,
        }
    }

    /// Creates a new event queue handle that is not connected to a shutdown flag.
    ///
    /// This method is used in tests, where we are never disabling shutdown warnings anyway.
    #[cfg(test)]
    pub(crate) fn without_shutdown(scheduler: &'static Scheduler<REv>) -> Self {
        EventQueueHandle::new(scheduler, SharedFlag::global_shared())
    }

    /// Schedule an event on a specific queue.
    ///
    /// The scheduled event will not have an ancestor.
    pub(crate) async fn schedule<Ev>(self, event: Ev, queue_kind: QueueKind)
    where
        REv: From<Ev>,
    {
        self.schedule_with_ancestor(None, event, queue_kind).await
    }

    /// Schedule an event on a specific queue.
    pub(crate) async fn schedule_with_ancestor<Ev>(
        self,
        ancestor: Option<NonZeroU64>,
        event: Ev,
        queue_kind: QueueKind,
    ) where
        REv: From<Ev>,
    {
        self.scheduler
            .push((ancestor, event.into()), queue_kind)
            .await
    }

    /// Returns number of events in each of the scheduler's queues.
    pub(crate) fn event_queues_counts(&self) -> HashMap<QueueKind, usize> {
        self.scheduler.event_queues_counts()
    }

    /// Returns whether the associated reactor is currently shutting down.
    pub(crate) fn shutdown_flag(&self) -> SharedFlag {
        self.is_shutting_down
    }
}

/// Reactor core.
///
/// Any reactor should implement this trait and be executed by the `reactor::run` function.
pub(crate) trait Reactor: Sized {
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
pub(crate) trait ReactorEvent: Send + Debug + From<ControlAnnouncement> + 'static {
    /// Returns the event as a control announcement, if possible.
    ///
    /// Returns a reference to a wrapped
    /// [`ControlAnnouncement`](`crate::effect::announcements::ControlAnnouncement`) if the event
    /// is indeed a control announcement variant.
    fn as_control(&self) -> Option<&ControlAnnouncement>;

    /// Converts the event into a control announcement without copying.
    ///
    /// Note that this function must return `Some` if and only `as_control` returns `Some`.
    fn try_into_control(self) -> Option<ControlAnnouncement>;

    /// Returns a cheap but human-readable description of the event.
    fn description(&self) -> &'static str {
        "anonymous event"
    }
}

/// A drop-like trait for `async` compatible drop-and-wait.
///
/// Shuts down a type by explicitly freeing resources, but allowing to wait on cleanup to complete.
pub(crate) trait Finalize: Sized {
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
/// The runner manages a reactor's event queue and reactor itself and can run it either continuously
/// or in a step-by-step manner.
#[derive(Debug)]
pub(crate) struct Runner<R>
where
    R: Reactor,
{
    /// The scheduler used for the reactor.
    scheduler: &'static Scheduler<R::Event>,

    /// The reactor instance itself.
    reactor: R,

    /// Counter for events, to aid tracing.
    current_event_id: u64,

    /// Timestamp of last reactor metrics update.
    last_metrics: Instant,

    /// Metrics for the runner.
    metrics: RunnerMetrics,

    /// Check if we need to update reactor metrics every this many events.
    event_metrics_threshold: u64,

    /// Only update reactor metrics if at least this much time has passed.
    event_metrics_min_delay: Duration,

    /// An accurate, possible TSC-supporting clock.
    clock: Clock,

    /// Flag indicating the reactor is being shut down.
    is_shutting_down: SharedFlag,
}

/// Metric data for the Runner
#[derive(Debug)]
struct RunnerMetrics {
    /// Total number of events processed.
    events: IntCounter,
    /// Histogram of how long it took to dispatch an event.
    event_dispatch_duration: Histogram,
    /// Total allocated RAM in bytes, as reported by stats_alloc.
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
        let events = IntCounter::new(
            "runner_events",
            "running total count of events handled by this reactor",
        )?;

        // Create an event dispatch histogram, putting extra emphasis on the area between 1-10 us.
        let event_dispatch_duration = Histogram::with_opts(
            HistogramOpts::new(
                "event_dispatch_duration",
                "time in nanoseconds to dispatch an event",
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
    /// Creates a new runner from a given configuration, using existing metrics.
    #[instrument("init", level = "debug", skip(cfg, rng, registry))]
    pub(crate) async fn with_metrics(
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
        let is_shutting_down = SharedFlag::new();

        let event_queue = EventQueueHandle::new(scheduler, is_shutting_down);
        let (reactor, initial_effects) = R::new(cfg, registry, event_queue, rng)?;

        // Run all effects from component instantiation.
        process_effects(None, scheduler, initial_effects)
            .instrument(debug_span!("process initial effects"))
            .await;

        info!("reactor main loop is ready");

        Ok(Runner {
            scheduler,
            reactor,
            current_event_id: 1,
            metrics: RunnerMetrics::new(registry)?,
            last_metrics: Instant::now(),
            event_metrics_min_delay: Duration::from_secs(30),
            event_metrics_threshold: 1000,
            clock: Clock::new(),
            is_shutting_down,
        })
    }

    /// Processes a single event on the event queue.
    ///
    /// Returns `false` if processing should stop.
    #[instrument("dispatch", level = "debug", fields(a, ev = self.current_event_id), skip(self, rng))]
    pub(crate) async fn crank(&mut self, rng: &mut NodeRng) -> bool {
        self.metrics.events.inc();

        let event_queue = EventQueueHandle::new(self.scheduler, self.is_shutting_down);
        let effect_builder = EffectBuilder::new(event_queue);

        // Update metrics like memory usage and event queue sizes.
        if self.current_event_id % self.event_metrics_threshold == 0 {
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
            }
        }

        let ((ancestor, event), queue) = self.scheduler.pop().await;
        trace!(%event, %queue, "current");
        let event_desc = event.description();

        // Create another span for tracing the processing of one event.
        Span::current().record("ev", &self.current_event_id);

        // If we know the ancestor of an event, record it.
        if let Some(ancestor) = ancestor {
            Span::current().record("a", &ancestor.get());
        }

        // Dispatch the event, then execute the resulting effect.
        let start = self.clock.start();

        let (effects, keep_going) = if event.as_control().is_some() {
            // We've received a control event, which will _not_ be handled by the reactor.
            match event.try_into_control() {
                None => {
                    // If `as_control().is_some()` is true, but `try_into_control` fails, the trait
                    // is implemented incorrectly.
                    error!(
                        "event::as_control succeeded, but try_into_control failed. this is a bug"
                    );

                    // We ignore the event.
                    (Default::default(), true)
                }
                Some(ControlAnnouncement::FatalError { file, line, msg }) => {
                    error!(%file, %line, %msg, "fatal error via control announcement");
                    (Default::default(), false)
                }
                Some(ControlAnnouncement::QueueDumpRequest {
                    dump_format,
                    finished,
                }) => {
                    match dump_format {
                        QueueDumpFormat::Serde(mut ser) => {
                            self.scheduler
                                .dump(move |queue_dump| {
                                    if let Err(err) =
                                        queue_dump.erased_serialize(&mut ser.as_serializer())
                                    {
                                        warn!(%err, "queue dump failed to serialize");
                                    }
                                })
                                .await;
                        }
                        QueueDumpFormat::Debug(ref file) => {
                            match file.try_clone() {
                                Ok(mut local_file) => {
                                    self.scheduler
                                        .dump(move |queue_dump| {
                                            write!(&mut local_file, "{:?}", queue_dump)
                                                .and_then(|_| local_file.flush())
                                                .map_err(|err| {
                                                    warn!(
                                                        ?err,
                                                        "failed to write/flush queue dump using debug format"
                                                    )
                                                })
                                                .ok();
                                        })
                                        .await;
                                }
                                Err(err) => warn!(
                                    %err,
                                    "could not create clone of temporary file for queue debug dump"
                                ),
                            };
                        }
                    }

                    // Notify requestor that we finished writing the queue dump.
                    finished.respond(()).await;

                    // Do nothing on queue dump otherwise.
                    (Default::default(), true)
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
            warn!(%event_desc, ns = delta.into_nanos(), "event took very long to dispatch");
        }
        self.metrics
            .event_dispatch_duration
            .observe(delta.into_nanos() as f64);

        // Run effects, with the current event ID as the ancestor for resulting set of events.
        process_effects(
            NonZeroU64::new(self.current_event_id),
            self.scheduler,
            effects,
        )
        .in_current_span()
        .await;

        self.current_event_id += 1;

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

        // mem_info gives us kilobytes
        let total = mem_info.total * 1024;
        let consumed = total - (mem_info.avail * 1024);

        let Stats {
            allocations: _,
            deallocations: _,
            reallocations: _,
            bytes_allocated,
            bytes_deallocated,
            bytes_reallocated: _,
        } = INSTRUMENTED_SYSTEM.stats();

        Some(AllocatedMem {
            allocated: bytes_allocated.saturating_sub(bytes_deallocated) as u64,
            consumed,
            total,
        })
    }

    /// Runs the reactor until `maybe_exit()` returns `Some` or we get interrupted by a termination
    /// signal.
    pub(crate) async fn run(&mut self, rng: &mut NodeRng) -> ReactorExit {
        loop {
            match TERMINATION_REQUESTED.load(Ordering::SeqCst) as i32 {
                0 => {
                    if let Some(reactor_exit) = self.reactor.maybe_exit() {
                        self.is_shutting_down.set();

                        // TODO: Workaround, until we actually use control announcements for
                        // exiting: Go over the entire remaining event queue and look for a control
                        // announcement. This approach is hacky, and should be replaced with
                        // `ControlAnnouncement` handling instead.
                        //
                        // When this workaround is fixed, we should revisit the handling of getting
                        // a deploy in the event stream server (handling of SseData::DeployAccepted)
                        // since that workaround of making two attempts with the first wrapped in a
                        // timeout should no longer be required.

                        for (ancestor, event) in
                            self.scheduler.drain_queue(QueueKind::Control).await
                        {
                            if let Some(ctrl_ann) = event.as_control() {
                                match ctrl_ann {
                                    ControlAnnouncement::FatalError { file, line, msg } => {
                                        warn!(%file, line=*line, %msg, "exiting due to fatal error scheduled before reactor completion");
                                        return ReactorExit::ProcessShouldExit(ExitCode::Abort);
                                    }
                                    ControlAnnouncement::QueueDumpRequest { .. } => {
                                        // Queue dumps are not handled when shutting down. TODO:
                                        // Maybe return an error instead, something like "reactor is
                                        // shutting down"?
                                    }
                                }
                            } else {
                                debug!(?ancestor, %event, "found non-control announcement while draining queue")
                            }
                        }

                        break reactor_exit;
                    }
                    if !self.crank(rng).await {
                        self.is_shutting_down.set();
                        break ReactorExit::ProcessShouldExit(ExitCode::Abort);
                    }
                }
                SIGINT => {
                    self.is_shutting_down.set();
                    break ReactorExit::ProcessShouldExit(ExitCode::SigInt);
                }
                SIGQUIT => {
                    self.is_shutting_down.set();
                    break ReactorExit::ProcessShouldExit(ExitCode::SigQuit);
                }
                SIGTERM => {
                    self.is_shutting_down.set();
                    break ReactorExit::ProcessShouldExit(ExitCode::SigTerm);
                }
                _ => error!("should be unreachable - bug in signal handler"),
            }
        }
    }

    /// Shuts down a reactor, sealing and draining the entire queue before returning it.
    pub(crate) async fn drain_into_inner(self) -> R {
        self.is_shutting_down.set();
        self.scheduler.seal();
        for (ancestor, event) in self.scheduler.drain_queues().await {
            debug!(?ancestor, %event, "drained event");
        }
        self.reactor
    }
}

#[cfg(test)]
impl<R> Runner<R>
where
    R: Reactor,
    R::Event: Serialize,
    R::Error: From<prometheus::Error>,
{
    /// Creates a new runner from a given configuration.
    ///
    /// Creates a metrics registry that is only going to be used in this runner.
    pub(crate) async fn new(cfg: R::Config, rng: &mut NodeRng) -> Result<Self, R::Error> {
        // Instantiate a new registry for metrics for this reactor.
        let registry = Registry::new();
        Self::with_metrics(cfg, rng, &registry).await
    }

    /// Inject (schedule then process) effects created via a call to `create_effects` which is
    /// itself passed an instance of an `EffectBuilder`.
    #[cfg(test)]
    pub(crate) async fn process_injected_effects<F>(&mut self, create_effects: F)
    where
        F: FnOnce(EffectBuilder<R::Event>) -> Effects<R::Event>,
    {
        let event_queue = EventQueueHandle::new(self.scheduler, self.is_shutting_down);
        let effect_builder = EffectBuilder::new(event_queue);

        let effects = create_effects(effect_builder);

        process_effects(None, self.scheduler, effects)
            .instrument(debug_span!(
                "process injected effects",
                ev = self.current_event_id
            ))
            .await;
    }

    /// Processes a single event if there is one, returns `None` otherwise.
    pub(crate) async fn try_crank(&mut self, rng: &mut NodeRng) -> Option<bool> {
        if self.scheduler.item_count() == 0 {
            None
        } else {
            Some(self.crank(rng).await)
        }
    }

    /// Returns a reference to the reactor.
    pub(crate) fn reactor(&self) -> &R {
        &self.reactor
    }

    /// Returns a mutable reference to the reactor.
    pub(crate) fn reactor_mut(&mut self) -> &mut R {
        &mut self.reactor
    }
}

#[cfg(test)]
impl Runner<InitializerReactor> {
    pub(crate) async fn new_with_chainspec(
        cfg: <InitializerReactor as Reactor>::Config,
        chainspec: Arc<Chainspec>,
        chainspec_raw_bytes: Arc<ChainspecRawBytes>,
    ) -> Result<Self, <InitializerReactor as Reactor>::Error> {
        let registry = Registry::new();
        let scheduler = utils::leak(Scheduler::new(QueueKind::weights()));

        let is_shutting_down = SharedFlag::new();
        let event_queue = EventQueueHandle::new(scheduler, is_shutting_down);
        let (reactor, initial_effects) = InitializerReactor::new_with_chainspec(
            cfg,
            &registry,
            event_queue,
            chainspec,
            chainspec_raw_bytes,
        )?;

        // Run all effects from component instantiation.
        let span = debug_span!("process initial effects");
        process_effects(None, scheduler, initial_effects)
            .instrument(span)
            .await;

        info!("reactor main loop is ready");

        let event_metrics_min_delay = Duration::from_secs(30);
        let now = Instant::now();
        Ok(Runner {
            scheduler,
            reactor,
            // It is important to initial event count to 1, as we use an ancestor event of 0
            // to mean "no ancestor".
            current_event_id: 1,
            metrics: RunnerMetrics::new(&registry)?,
            // Calculate the `last_metrics` timestamp to be exactly one delay in the past. This will
            // cause the runner to collect metrics at the first opportunity.
            last_metrics: now.checked_sub(event_metrics_min_delay).unwrap_or(now),
            event_metrics_min_delay,
            event_metrics_threshold: 1000,
            clock: Clock::new(),
            is_shutting_down,
        })
    }
}

/// Spawns tasks that will process the given effects.
///
/// Result events from processing the events will be scheduled with the given ancestor.
async fn process_effects<Ev>(
    ancestor: Option<NonZeroU64>,
    scheduler: &'static Scheduler<Ev>,
    effects: Effects<Ev>,
) where
    Ev: Send + 'static,
{
    // TODO: Properly carry around priorities.
    let queue_kind = QueueKind::default();

    for effect in effects {
        tokio::spawn(async move {
            for event in effect.await {
                scheduler.push((ancestor, event), queue_kind).await
            }
        });
    }
}

/// Converts a single effect into another by wrapping it.
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
pub(crate) fn wrap_effects<Ev, REv, F>(wrap: F, effects: Effects<Ev>) -> Effects<REv>
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

pub(crate) fn handle_fetch_response<R, T>(
    reactor: &mut R,
    effect_builder: EffectBuilder<<R as Reactor>::Event>,
    rng: &mut NodeRng,
    sender: NodeId,
    serialized_item: &[u8],
    verifiable_chunked_hash_activation: EraId,
) -> Effects<<R as Reactor>::Event>
where
    T: Item,
    R: Reactor,
    <R as Reactor>::Event: From<fetcher::Event<T>> + From<BlocklistAnnouncement>,
{
    match fetcher::Event::<T>::from_get_response_serialized_item(
        sender,
        serialized_item,
        verifiable_chunked_hash_activation,
    ) {
        Some(fetcher_event) => {
            Reactor::dispatch_event(reactor, effect_builder, rng, fetcher_event.into())
        }
        None => {
            info!(
                "{} sent us a {:?} item we couldn't parse, banning peer",
                sender,
                T::TAG
            );
            effect_builder
                .announce_disconnect_from_peer(sender)
                .ignore()
        }
    }
}
