//! Testing utilities.
//!
//! Contains various parts and components to aid writing tests and simulations using the
//! `casper-node` library.

mod condition_check_reactor;
mod fake_transaction_acceptor;
pub(crate) mod filter_reactor;
pub(crate) mod network;
pub(crate) mod test_clock;

use std::{
    any::type_name,
    fmt::Debug,
    fs,
    io::Write,
    marker::PhantomData,
    ops::Range,
    sync::atomic::{AtomicU16, Ordering},
    time,
};

use anyhow::Context;
use assert_json_diff::{assert_json_eq, assert_json_matches_no_panic, CompareMode, Config};
use derive_more::From;
use futures::channel::oneshot;
use once_cell::sync::Lazy;
use rand::Rng;
use schemars::schema::RootSchema;
use serde_json::Value;
use tempfile::TempDir;
use tokio::runtime::{self, Runtime};
use tracing::{debug, warn};

use casper_types::{testing::TestRng, Deploy, TimeDiff, Timestamp};

use crate::{
    components::Component,
    effect::{
        announcements::{ControlAnnouncement, FatalAnnouncement},
        requests::NetworkRequest,
        EffectBuilder, Effects, Responder,
    },
    logging,
    protocol::Message,
    reactor::{EventQueueHandle, QueueKind, ReactorEvent, Scheduler},
};
pub(crate) use condition_check_reactor::ConditionCheckReactor;
pub(crate) use fake_transaction_acceptor::FakeTransactionAcceptor;

/// Time to wait (at most) for a `fatal` to resolve before considering the dropping of a responder a
/// problem.
const FATAL_GRACE_TIME: time::Duration = time::Duration::from_secs(3);

/// The range of ports used to allocate ports for network ports.
///
/// The IANA ephemeral port range is 49152–65535, while Linux uses 32768–60999 by default. Windows
/// on the other hand uses 1025–60000. Mac OS X seems to use 49152-65535. For this reason this
/// constant uses different values on different systems.

// Note: Ensure the range is prime, so that any chosen `TEST_PORT_STRIDE` wraps around without
// conflicting.

// All reasonable non-Windows systems seem to have a "hole" just below port 30000.
//
// This also does not conflict with nctl ports.
#[cfg(not(target_os = "windows"))]
const TEST_PORT_RANGE: Range<u16> = 29000..29997;

// On windows, we sneak into the upper end instead.
#[cfg(target_os = "windows")]
const TEST_PORT_RANGE: Range<u16> = 60001..60998;

/// Random offset + stride for port generation.
const TEST_PORT_STRIDE: u16 = 29;

/// Create an unused port on localhost.
///
/// Returns a random port on localhost, provided that no other applications are binding ports inside
/// `TEST_PORT_RANGE` and no other testing process is run in parallel. Should the latter happen,
/// some randomization is used to avoid conflicts, without guarantee of success.
pub(crate) fn unused_port_on_localhost() -> u16 {
    // Previous iterations of this implementation tried other approaches such as binding an
    // ephemeral port and using that. This ran into race condition issues when the port was reused
    // in the timespan where it was released and rebound.

    // The simpler approach is to select a random port from the non-ephemeral range and hope that no
    // daemons are already bound/listening on it, which should not be the case on a CI system.

    // We use a random offset and stride to stretch this a little bit, should two processes run at
    // the same time.
    static NEXT_PORT: Lazy<AtomicU16> = Lazy::new(|| {
        rand::thread_rng()
            .gen_range(TEST_PORT_RANGE.start..(TEST_PORT_RANGE.start + TEST_PORT_STRIDE))
            .into()
    });

    NEXT_PORT.fetch_add(TEST_PORT_STRIDE, Ordering::SeqCst)
}

/// Sets up logging for testing.
///
/// Can safely be called multiple times.
pub(crate) fn init_logging() {
    // TODO: Write logs to file by default for each test.
    logging::init()
        // Ignore the return value, setting the global subscriber will fail if `init_logging` has
        // been called before, which we don't care about.
        .ok();
}

/// Harness to test a single component as isolated as possible.
///
/// Contains enough reactor machinery to drive a single component and a temporary directory.
///
/// # Usage
///
/// Construction of a harness can be done straightforwardly through the `Default` trait, or the
/// builder can be used to construct various aspects of it.
pub(crate) struct ComponentHarness<REv: 'static> {
    /// Test random number generator instance.
    pub(crate) rng: TestRng,
    /// Scheduler for events. Only explicitly polled by the harness.
    pub(crate) scheduler: &'static Scheduler<REv>,
    /// Effect builder pointing at the scheduler.
    pub(crate) effect_builder: EffectBuilder<REv>,
    /// A temporary directory that can be used to store various data.
    pub(crate) tmp: TempDir,
    /// The `async` runtime used to execute effects.
    pub(crate) runtime: Runtime,
}

/// Builder for a `ComponentHarness`.
pub(crate) struct ComponentHarnessBuilder<REv: 'static> {
    rng: Option<TestRng>,
    tmp: Option<TempDir>,
    _phantom: PhantomData<REv>,
}

impl<REv: 'static + Debug> ComponentHarnessBuilder<REv> {
    /// Builds a component harness instance.
    ///
    /// # Panics
    ///
    /// Panics if building the harness fails.
    pub(crate) fn build(self) -> ComponentHarness<REv> {
        self.try_build().expect("failed to build component harness")
    }

    /// Sets the on-disk harness folder.
    pub(crate) fn on_disk(mut self, on_disk: TempDir) -> ComponentHarnessBuilder<REv> {
        self.tmp = Some(on_disk);
        self
    }

    /// Sets the test random number generator.
    pub(crate) fn rng(mut self, rng: TestRng) -> ComponentHarnessBuilder<REv> {
        self.rng = Some(rng);
        self
    }

    /// Tries to build a component harness.
    ///
    /// Construction may fail for various reasons such as not being able to create a temporary
    /// directory.
    pub(crate) fn try_build(self) -> anyhow::Result<ComponentHarness<REv>> {
        let tmp = match self.tmp {
            Some(tmp) => tmp,
            None => {
                TempDir::new().context("could not create temporary directory for test harness")?
            }
        };

        let rng = self.rng.unwrap_or_else(TestRng::new);

        let scheduler = Box::leak(Box::new(Scheduler::new(QueueKind::weights(), None)));
        let event_queue_handle = EventQueueHandle::without_shutdown(scheduler);
        let effect_builder = EffectBuilder::new(event_queue_handle);
        let runtime = runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("build tokio runtime")?;

        Ok(ComponentHarness {
            rng,
            scheduler,
            effect_builder,
            tmp,
            runtime,
        })
    }
}

impl<REv: 'static> ComponentHarness<REv> {
    /// Creates a new component harness builder.
    pub(crate) fn builder() -> ComponentHarnessBuilder<REv> {
        ComponentHarnessBuilder {
            rng: None,
            tmp: None,
            _phantom: PhantomData,
        }
    }

    /// Deconstructs the harness, keeping the on-disk state and test rng.
    pub(crate) fn into_parts(self) -> (TempDir, TestRng) {
        (self.tmp, self.rng)
    }

    /// Returns whether or not there are pending events on the event queue.
    pub(crate) fn is_idle(&self) -> bool {
        self.scheduler.item_count() == 0
    }

    /// Sends a request, expecting an immediate response.
    ///
    /// Sends a request by creating a channel for the response, then mapping it using the function
    /// `f`. Executes all returned effects, then awaits a response.
    pub(crate) fn send_request<C, T, F>(&mut self, component: &mut C, f: F) -> T
    where
        C: Component<REv>,
        <C as Component<REv>>::Event: Send + 'static,
        T: Send + 'static,
        F: FnOnce(Responder<T>) -> C::Event,
        REv: ReactorEvent,
    {
        // Prepare a channel.
        let (sender, receiver) = oneshot::channel();

        // Create response function.
        let responder = Responder::without_shutdown(sender);

        // Create the event for the component.
        let request_event = f(responder);

        // Send directly to component.
        let returned_effects = self.send_event(component, request_event);

        // Execute the effects on our dedicated runtime, hopefully creating the responses.
        let mut join_handles = Vec::new();
        for effect in returned_effects {
            join_handles.push(self.runtime.spawn(effect));
        }

        // Wait for a response to arrive.
        self.runtime.block_on(receiver).unwrap_or_else(|err| {
            // A channel was closed and this is usually an error. However, we consider all pending
            // events, in case we did get a control announcement requiring us to fatal error instead
            // before panicking on the basis of the missing response.

            // We give each of them a little time to produce the desired event. Note that `join_all`
            // should be safe to cancel, since we are only awaiting join handles.
            let join_all = async {
                for handle in join_handles {
                    if let Err(err) = handle.await {
                        warn!("Join error while waiting for an effect to finish: {}", err);
                    };
                }
            };

            if let Err(_timeout) = self.runtime.block_on(async move {
                // Note: timeout can only be called from within a running running, this is why
                // we use an extra `async` block here.
                tokio::time::timeout(FATAL_GRACE_TIME, join_all).await
            }) {
                warn!(grace_time=?FATAL_GRACE_TIME, "while a responder was dropped in a unit test, \
                I waited for all other pending effects to complete in case the output of a \
                `fatal!` was among them but none of them completed");
            }

            // Iterate over all events that currently are inside the queue and fish out any fatal.
            for _ in 0..(self.scheduler.item_count()) {
                let ((_ancestor, ev), _queue_kind) = self.runtime.block_on(self.scheduler.pop());

                if !ev.is_control() {
                    debug!(?ev, "ignoring event while looking for a fatal");
                    continue;
                }
                match ev.try_into_control().unwrap() {
                    ControlAnnouncement::ShutdownDueToUserRequest { .. } => {
                        panic!("a control announcement requesting a shutdown due to user request was received")
                    }
                    ControlAnnouncement::ShutdownForUpgrade { .. } => {
                        panic!("a control announcement requesting a shutdown for upgrade was received")
                    }
                    fatal @ ControlAnnouncement::FatalError { .. } => {
                        panic!(
                            "a control announcement requesting a fatal error was received: {}",
                            fatal
                        )
                    }
                    ControlAnnouncement::QueueDumpRequest { .. } => {
                        panic!("queue dumps are not supported in the test harness")
                    }
                }
            }

            // Barring a `fatal`, the channel should never be closed, ever.
            panic!(
                "request for {} channel closed with return value \"{}\" in unit test harness",
                type_name::<T>(),
                err,
            );
        })
    }

    /// Sends a single event to a component, returning the created effects.
    #[inline]
    pub(crate) fn send_event<C>(&mut self, component: &mut C, ev: C::Event) -> Effects<C::Event>
    where
        C: Component<REv>,
    {
        component.handle_event(self.effect_builder, &mut self.rng, ev)
    }
}

impl<REv: 'static + Debug> Default for ComponentHarness<REv> {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// A special event for unit tests.
///
/// Essentially discards most events (they are not even processed by the unit testing harness),
/// except for control announcements, which are preserved.
#[derive(Debug, From)]
pub(crate) enum UnitTestEvent {
    /// A preserved control announcement.
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    #[from]
    FatalAnnouncement(FatalAnnouncement),
    /// A network request made by the component under test.
    #[from]
    NetworkRequest(NetworkRequest<Message>),
}

impl ReactorEvent for UnitTestEvent {
    fn is_control(&self) -> bool {
        match self {
            UnitTestEvent::ControlAnnouncement(_) | UnitTestEvent::FatalAnnouncement(_) => true,
            UnitTestEvent::NetworkRequest(_) => false,
        }
    }

    fn try_into_control(self) -> Option<ControlAnnouncement> {
        match self {
            UnitTestEvent::ControlAnnouncement(ctrl_ann) => Some(ctrl_ann),
            UnitTestEvent::FatalAnnouncement(FatalAnnouncement { file, line, msg }) => {
                Some(ControlAnnouncement::FatalError { file, line, msg })
            }
            UnitTestEvent::NetworkRequest(_) => None,
        }
    }
}

/// Helper function to simulate the passage of time.
pub(crate) async fn advance_time(duration: time::Duration) {
    tokio::time::pause();
    tokio::time::advance(duration).await;
    tokio::time::resume();
    debug!("advanced time by {} secs", duration.as_secs());
}

/// Creates a test deploy created at given instant and with given ttl.
pub(crate) fn create_test_deploy(
    created_ago: TimeDiff,
    ttl: TimeDiff,
    now: Timestamp,
    test_rng: &mut TestRng,
) -> Deploy {
    Deploy::random_with_timestamp_and_ttl(test_rng, now - created_ago, ttl)
}

/// Creates a random deploy that is considered expired.
pub(crate) fn create_expired_deploy(now: Timestamp, test_rng: &mut TestRng) -> Deploy {
    create_test_deploy(
        TimeDiff::from_seconds(20),
        TimeDiff::from_seconds(10),
        now,
        test_rng,
    )
}

// TODO - remove `allow` once used in tests again.
#[allow(dead_code)]
/// Creates a random deploy that is considered not expired.
pub(crate) fn create_not_expired_deploy(now: Timestamp, test_rng: &mut TestRng) -> Deploy {
    create_test_deploy(
        TimeDiff::from_seconds(20),
        TimeDiff::from_seconds(60),
        now,
        test_rng,
    )
}

/// Assert that the file at `schema_path` matches the provided `RootSchema`, which can be derived
/// from `schemars::schema_for!` or `schemars::schema_for_value!`, for example. This method will
/// create a temporary file with the actual schema and print the location if it fails.
pub fn assert_schema(schema_path: &str, actual_schema: RootSchema) {
    let expected_schema = fs::read_to_string(schema_path).unwrap();
    let expected_schema: Value = serde_json::from_str(&expected_schema).unwrap();
    let actual_schema = serde_json::to_string_pretty(&actual_schema).unwrap();
    let mut temp_file = tempfile::Builder::new()
        .suffix(".json")
        .tempfile_in(env!("OUT_DIR"))
        .unwrap();
    temp_file.write_all(actual_schema.as_bytes()).unwrap();
    let actual_schema: Value = serde_json::from_str(&actual_schema).unwrap();
    let (_file, temp_file_path) = temp_file.keep().unwrap();

    let result = assert_json_matches_no_panic(
        &actual_schema,
        &expected_schema,
        Config::new(CompareMode::Strict),
    );
    assert_eq!(
        result,
        Ok(()),
        "schema does not match:\nexpected:\n{}\nactual:\n{}\n",
        schema_path,
        temp_file_path.display()
    );
    assert_json_eq!(actual_schema, expected_schema);
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{unused_port_on_localhost, ComponentHarness};

    #[test]
    fn default_works_without_panicking_for_component_harness() {
        let _harness = ComponentHarness::<()>::default();
    }

    #[test]
    fn can_generate_at_least_100_unused_ports() {
        let ports: HashSet<u16> = (0..100).map(|_| unused_port_on_localhost()).collect();

        assert_eq!(ports.len(), 100);
    }
}
