#![allow(clippy::boxed_local)] // We use boxed locals to pass on event data unchanged.

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
pub(crate) mod main_reactor;
mod queue_kind;

use std::{
    any,
    collections::HashMap,
    env,
    fmt::{Debug, Display},
    io::Write,
    mem,
    num::NonZeroU64,
    str::FromStr,
    sync::{atomic::Ordering, Arc},
};

use datasize::DataSize;
use erased_serde::Serialize as ErasedSerialize;
#[cfg(test)]
use fake_instant::FakeClock;
use futures::{future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use prometheus::{self, Histogram, HistogramOpts, IntCounter, IntGauge, Registry};
use quanta::{Clock, IntoNanoseconds};
use serde::Serialize;
use signal_hook::consts::signal::{SIGINT, SIGQUIT, SIGTERM};
use stats_alloc::{Stats, INSTRUMENTED_SYSTEM};
use tokio::time::{Duration, Instant};
use tracing::{debug_span, error, info, instrument, trace, warn, Span};
use tracing_futures::Instrument;

#[cfg(test)]
use casper_types::testing::TestRng;
use casper_types::{Block, BlockHeader, Chainspec, ChainspecRawBytes, Deploy, FinalitySignature};

#[cfg(target_os = "linux")]
use utils::rlimit::{Limit, OpenFiles, ResourceLimit};

#[cfg(test)]
use crate::testing::{network::NetworkedReactor, ConditionCheckReactor};
use crate::{
    components::{
        block_accumulator,
        fetcher::{self, FetchItem},
        network::{blocklist::BlocklistJustification, Identity as NetworkIdentity},
        transaction_acceptor,
    },
    effect::{
        announcements::{ControlAnnouncement, PeerBehaviorAnnouncement, QueueDumpFormat},
        incoming::NetResponse,
        Effect, EffectBuilder, EffectExt, Effects,
    },
    types::{
        ApprovalsHashes, BlockExecutionResultsOrChunk, ExitCode, LegacyDeploy, NodeId, SyncLeap,
        TrieOrChunk,
    },
    unregister_metric,
    utils::{self, SharedFlag, WeightedRoundRobin},
    NodeRng, TERMINATION_REQUESTED,
};
pub(crate) use queue_kind::QueueKind;

/// Default threshold for when an event is considered slow.  Can be overridden by setting the env
/// var `CL_EVENT_MAX_MICROSECS=<MICROSECONDS>`.
const DEFAULT_DISPATCH_EVENT_THRESHOLD: Duration = Duration::from_secs(1);
const DISPATCH_EVENT_THRESHOLD_ENV_VAR: &str = "CL_EVENT_MAX_MICROSECS";
#[cfg(test)]
const POLL_INTERVAL: Duration = Duration::from_millis(10);

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
                    tracing::debug!(?new_limit, "successfully increased open files limit");
                }
            } else {
                tracing::debug!(
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
        chainspec: Arc<Chainspec>,
        chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        network_identity: NetworkIdentity,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error>;

    /// Instructs the reactor to update performance metrics, if any.
    fn update_metrics(&mut self, _event_queue_handle: EventQueueHandle<Self::Event>) {}
}

/// A reactor event type.
pub(crate) trait ReactorEvent: Send + Debug + From<ControlAnnouncement> + 'static {
    /// Returns `true` if the event is a control announcement variant.
    fn is_control(&self) -> bool;

    /// Converts the event into a control announcement without copying.
    ///
    /// Note that this function must return `Some` if and only `is_control` returns `true`.
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
    #[instrument(
        "init",
        level = "debug",
        skip_all,
        fields(node_id = %NodeId::from(&network_identity))
    )]
    pub(crate) async fn with_metrics(
        cfg: R::Config,
        chainspec: Arc<Chainspec>,
        chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        network_identity: NetworkIdentity,
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

        let event_queue_dump_threshold =
            env::var("CL_EVENT_QUEUE_DUMP_THRESHOLD").map_or(None, |s| s.parse::<usize>().ok());

        let scheduler = utils::leak(Scheduler::new(
            QueueKind::weights(),
            event_queue_dump_threshold,
        ));
        let is_shutting_down = SharedFlag::new();
        let event_queue = EventQueueHandle::new(scheduler, is_shutting_down);
        let (reactor, initial_effects) = R::new(
            cfg,
            chainspec,
            chainspec_raw_bytes,
            network_identity,
            registry,
            event_queue,
            rng,
        )?;

        info!(
            "Reactor: with_metrics has: {} initial_effects",
            initial_effects.len()
        );
        // Run all effects from component instantiation.
        process_effects(None, scheduler, initial_effects, QueueKind::Regular)
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
    /// Returns `Some(exit_code)` if processing should stop.
    #[instrument("dispatch", level = "debug", fields(a, ev = self.current_event_id), skip(self, rng))]
    pub(crate) async fn crank(&mut self, rng: &mut NodeRng) -> Option<ExitCode> {
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
                trace!(%allocated, %total, "memory allocated");
                self.metrics.allocated_ram_bytes.set(allocated as i64);
                self.metrics.consumed_ram_bytes.set(consumed as i64);
                self.metrics.total_ram_bytes.set(total as i64);
            }
        }

        let ((ancestor, event), queue_kind) = self.scheduler.pop().await;
        trace!(%event, %queue_kind, "current");
        let event_desc = event.description();

        // Create another span for tracing the processing of one event.
        Span::current().record("ev", self.current_event_id);

        // If we know the ancestor of an event, record it.
        if let Some(ancestor) = ancestor {
            Span::current().record("a", ancestor.get());
        }

        // Dispatch the event, then execute the resulting effect.
        let start = self.clock.start();

        let (effects, maybe_exit_code, queue_kind) = if event.is_control() {
            // We've received a control event, which will _not_ be handled by the reactor.
            match event.try_into_control() {
                None => {
                    // If `as_control().is_some()` is true, but `try_into_control` fails, the trait
                    // is implemented incorrectly.
                    error!(
                        "event::as_control succeeded, but try_into_control failed. this is a bug"
                    );

                    // We ignore the event.
                    (Effects::new(), None, QueueKind::Control)
                }
                Some(ControlAnnouncement::ShutdownDueToUserRequest) => (
                    Effects::new(),
                    Some(ExitCode::CleanExitDontRestart),
                    QueueKind::Control,
                ),
                Some(ControlAnnouncement::ShutdownForUpgrade) => {
                    (Effects::new(), Some(ExitCode::Success), QueueKind::Control)
                }
                Some(ControlAnnouncement::FatalError { file, line, msg }) => {
                    error!(%file, %line, %msg, "fatal error via control announcement");
                    (Effects::new(), Some(ExitCode::Abort), QueueKind::Control)
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

                    // Notify requester that we finished writing the queue dump.
                    finished.respond(()).await;

                    // Do nothing on queue dump otherwise.
                    (Default::default(), None, QueueKind::Control)
                }
            }
        } else {
            (
                self.reactor.dispatch_event(effect_builder, rng, event),
                None,
                queue_kind,
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
            queue_kind,
        )
        .in_current_span()
        .await;

        self.current_event_id += 1;

        maybe_exit_code
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

    /// Runs the reactor until `self.crank` returns `Some` or we get interrupted by a termination
    /// signal.
    pub(crate) async fn run(&mut self, rng: &mut NodeRng) -> ExitCode {
        loop {
            match TERMINATION_REQUESTED.load(Ordering::SeqCst) as i32 {
                0 => {
                    if let Some(exit_code) = self.crank(rng).await {
                        self.is_shutting_down.set();
                        break exit_code;
                    }
                }
                SIGINT => {
                    self.is_shutting_down.set();
                    break ExitCode::SigInt;
                }
                SIGQUIT => {
                    self.is_shutting_down.set();
                    break ExitCode::SigQuit;
                }
                SIGTERM => {
                    self.is_shutting_down.set();
                    break ExitCode::SigTerm;
                }
                _ => error!("should be unreachable - bug in signal handler"),
            }
        }
    }
}

#[cfg(test)]
#[derive(Eq, PartialEq, Debug)]
pub(crate) enum TryCrankOutcome {
    NoEventsToProcess,
    ProcessedAnEvent,
    ShouldExit(ExitCode),
    Exited,
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
    pub(crate) async fn new(
        cfg: R::Config,
        chainspec: Arc<Chainspec>,
        chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        rng: &mut NodeRng,
    ) -> Result<Self, R::Error> {
        // Instantiate a new registry for metrics for this reactor.
        let registry = Registry::new();
        let network_identity = NetworkIdentity::with_generated_certs().unwrap();
        Self::with_metrics(
            cfg,
            chainspec,
            chainspec_raw_bytes,
            network_identity,
            rng,
            &registry,
        )
        .await
    }

    /// Create an instance of an `EffectBuilder`.
    #[cfg(test)]
    pub(crate) fn effect_builder(&self) -> EffectBuilder<R::Event> {
        let event_queue = EventQueueHandle::new(self.scheduler, self.is_shutting_down);
        EffectBuilder::new(event_queue)
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

        process_effects(None, self.scheduler, effects, QueueKind::Regular)
            .instrument(debug_span!(
                "process injected effects",
                ev = self.current_event_id
            ))
            .await;
    }

    /// Processes a single event if there is one and we haven't previously handled an exit code.
    pub(crate) async fn try_crank(&mut self, rng: &mut NodeRng) -> TryCrankOutcome {
        if self.is_shutting_down.is_set() {
            TryCrankOutcome::Exited
        } else if self.scheduler.item_count() == 0 {
            TryCrankOutcome::NoEventsToProcess
        } else {
            match self.crank(rng).await {
                Some(exit_code) => {
                    self.is_shutting_down.set();
                    TryCrankOutcome::ShouldExit(exit_code)
                }
                None => TryCrankOutcome::ProcessedAnEvent,
            }
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

    /// Shuts down a reactor, sealing and draining the entire queue before returning it.
    pub(crate) async fn drain_into_inner(self) -> R {
        self.is_shutting_down.set();
        self.scheduler.seal();
        for (ancestor, event) in self.scheduler.drain_queues().await {
            tracing::debug!(?ancestor, %event, "drained event");
        }
        self.reactor
    }
}

#[cfg(test)]
impl<R> Runner<ConditionCheckReactor<R>>
where
    R: Reactor + NetworkedReactor,
    R::Event: Serialize,
    R::Error: From<prometheus::Error>,
{
    /// Cranks the runner until `condition` is true or until `within` has elapsed.
    ///
    /// Returns `true` if `condition` has been met within the specified timeout.
    ///
    /// Panics if cranking causes the node to return an exit code.
    pub(crate) async fn crank_until<F>(&mut self, rng: &mut TestRng, condition: F, within: Duration)
    where
        F: Fn(&R::Event) -> bool + Send + 'static,
    {
        self.reactor.set_condition_checker(Box::new(condition));

        tokio::time::timeout(within, self.crank_and_check_indefinitely(rng))
            .await
            .unwrap_or_else(|_| {
                panic!(
                    "Runner::crank_until() timed out after {}s on node {}",
                    within.as_secs_f64(),
                    self.reactor.inner().node_id()
                )
            })
    }

    async fn crank_and_check_indefinitely(&mut self, rng: &mut TestRng) {
        loop {
            match self.try_crank(rng).await {
                TryCrankOutcome::NoEventsToProcess => {
                    FakeClock::advance_time(POLL_INTERVAL.as_millis() as u64);
                    tokio::time::sleep(POLL_INTERVAL).await;
                    continue;
                }
                TryCrankOutcome::ProcessedAnEvent => {}
                TryCrankOutcome::ShouldExit(exit_code) => {
                    panic!("should not exit: {:?}", exit_code)
                }
                TryCrankOutcome::Exited => unreachable!(),
            }

            if self.reactor.condition_result() {
                info!("{} met condition", self.reactor.inner().node_id());
                return;
            }
        }
    }
}

/// Spawns tasks that will process the given effects.
///
/// Result events from processing the events will be scheduled with the given ancestor.
async fn process_effects<Ev>(
    ancestor: Option<NonZeroU64>,
    scheduler: &'static Scheduler<Ev>,
    effects: Effects<Ev>,
    queue_kind: QueueKind,
) where
    Ev: Send + 'static,
{
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

fn handle_fetch_response<R, I>(
    reactor: &mut R,
    effect_builder: EffectBuilder<<R as Reactor>::Event>,
    rng: &mut NodeRng,
    sender: NodeId,
    serialized_item: &[u8],
) -> Effects<<R as Reactor>::Event>
where
    I: FetchItem,
    R: Reactor,
    <R as Reactor>::Event: From<fetcher::Event<I>> + From<PeerBehaviorAnnouncement>,
{
    match fetcher::Event::<I>::from_get_response_serialized_item(sender, serialized_item) {
        Some(fetcher_event) => {
            Reactor::dispatch_event(reactor, effect_builder, rng, fetcher_event.into())
        }
        None => effect_builder
            .announce_block_peer_with_justification(
                sender,
                BlocklistJustification::SentBadItem { tag: I::TAG },
            )
            .ignore(),
    }
}

fn handle_get_response<R>(
    reactor: &mut R,
    effect_builder: EffectBuilder<<R as Reactor>::Event>,
    rng: &mut NodeRng,
    sender: NodeId,
    message: Box<NetResponse>,
) -> Effects<<R as Reactor>::Event>
where
    R: Reactor,
    <R as Reactor>::Event: From<transaction_acceptor::Event>
        + From<fetcher::Event<FinalitySignature>>
        + From<fetcher::Event<Block>>
        + From<fetcher::Event<BlockHeader>>
        + From<fetcher::Event<BlockExecutionResultsOrChunk>>
        + From<fetcher::Event<LegacyDeploy>>
        + From<fetcher::Event<Deploy>>
        + From<fetcher::Event<SyncLeap>>
        + From<fetcher::Event<TrieOrChunk>>
        + From<fetcher::Event<ApprovalsHashes>>
        + From<block_accumulator::Event>
        + From<PeerBehaviorAnnouncement>,
{
    match *message {
        NetResponse::Deploy(ref serialized_item) => handle_fetch_response::<R, Deploy>(
            reactor,
            effect_builder,
            rng,
            sender,
            serialized_item,
        ),
        NetResponse::LegacyDeploy(ref serialized_item) => handle_fetch_response::<R, LegacyDeploy>(
            reactor,
            effect_builder,
            rng,
            sender,
            serialized_item,
        ),
        NetResponse::Block(ref serialized_item) => {
            handle_fetch_response::<R, Block>(reactor, effect_builder, rng, sender, serialized_item)
        }
        NetResponse::BlockHeader(ref serialized_item) => handle_fetch_response::<R, BlockHeader>(
            reactor,
            effect_builder,
            rng,
            sender,
            serialized_item,
        ),
        NetResponse::FinalitySignature(ref serialized_item) => {
            handle_fetch_response::<R, FinalitySignature>(
                reactor,
                effect_builder,
                rng,
                sender,
                serialized_item,
            )
        }
        NetResponse::SyncLeap(ref serialized_item) => handle_fetch_response::<R, SyncLeap>(
            reactor,
            effect_builder,
            rng,
            sender,
            serialized_item,
        ),
        NetResponse::ApprovalsHashes(ref serialized_item) => {
            handle_fetch_response::<R, ApprovalsHashes>(
                reactor,
                effect_builder,
                rng,
                sender,
                serialized_item,
            )
        }
        NetResponse::BlockExecutionResults(ref serialized_item) => {
            handle_fetch_response::<R, BlockExecutionResultsOrChunk>(
                reactor,
                effect_builder,
                rng,
                sender,
                serialized_item,
            )
        }
    }
}
