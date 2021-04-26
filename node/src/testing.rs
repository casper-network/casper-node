//! Testing utilities.
//!
//! Contains various parts and components to aid writing tests and simulations using the
//! `casper-node` library.

mod condition_check_reactor;
mod multi_stage_test_reactor;
pub mod network;
mod test_rng;

use std::{
    any::type_name,
    fmt::Debug,
    marker::PhantomData,
    net::{Ipv4Addr, TcpListener},
    time,
};

use anyhow::Context;
use derive_more::From;
use futures::channel::oneshot;
use serde::{de::DeserializeOwned, Serialize};
use tempfile::TempDir;
use tokio::runtime::{self, Runtime};
use tracing::{debug, info, warn};

use crate::{
    components::Component,
    effect::{announcements::ControlAnnouncement, EffectBuilder, Effects, Responder},
    logging,
    reactor::{EventQueueHandle, QueueKind, ReactorEvent, Scheduler},
};
pub(crate) use condition_check_reactor::ConditionCheckReactor;
pub(crate) use multi_stage_test_reactor::MultiStageTestReactor;
pub(crate) use test_rng::TestRng;

/// Time to wait (at most) for a `fatal` to resolve before considering the dropping of a responder a
/// problem.
const FATAL_GRACE_TIME: time::Duration = time::Duration::from_secs(3);

pub fn bincode_roundtrip<T: Serialize + DeserializeOwned + Eq + Debug>(value: &T) {
    let serialized = bincode::serialize(value).unwrap();
    let deserialized = bincode::deserialize(serialized.as_slice()).unwrap();
    assert_eq!(*value, deserialized);
}

/// Create an unused port on localhost.
pub(crate) fn unused_port_on_localhost() -> u16 {
    // Unfortunately a randomly generated port by a random number generator still has a chance to
    // hit the occasional duplicate or an already bound port once in a while, due to the small port
    // space. For this reason, we ask the OS for an unused port instead and hope that no one binds
    // to it in the meantime.

    // For a collision to occur, it is now required that after running this function, but before
    // rebinding the port, an unrelated program or a parallel running test must manage to bind to
    // precisely this port, hitting the same port randomly.

    // This is slightly better than a strictly random port, since it takes already bound ports
    // across the entire interface into account, but it does rely on the OS providing random ports
    // when asked for a _unused_ one.

    // An alternative approach is to create a bound port with `SO_REUSEPORT`, which would close the
    // gap between calling this function and binding again, never calling listening on the instance
    // created by this function, but still blocking it from being reassigned by accident. This
    // approach requires the networking component to either accept arbitrary incoming sockets to be
    // passed in or bind with `SO_REUSEPORT` as well, both are undesirable options. See
    // https://stackoverflow.com/questions/14388706/how-do-so-reuseaddr-and-so-reuseport-differ for
    // a detailed description on port reuse flags.

    let listener = TcpListener::bind((Ipv4Addr::new(127, 0, 0, 1), 0))
        .expect("could not bind new random port on localhost");
    let local_addr = listener
        .local_addr()
        .expect("local listener has no address?");

    let port = local_addr.port();
    info!(%port, "OS generated random localhost port");

    // Once we drop the listener, the port should be closed.
    port
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
    /// An event queue handle to the scheduler.
    #[allow(unused)] // TODO: Remove once in use.
    pub(crate) event_queue_handle: EventQueueHandle<REv>,
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

impl<REv: 'static> ComponentHarnessBuilder<REv> {
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

        let scheduler = Box::leak(Box::new(Scheduler::new(QueueKind::weights())));
        let event_queue_handle = EventQueueHandle::new(scheduler);
        let effect_builder = EffectBuilder::new(event_queue_handle);
        let runtime = runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .context("build tokio runtime")?;

        Ok(ComponentHarness {
            rng,
            scheduler,
            event_queue_handle,
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
        let responder = Responder::create(sender);

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
                let (ev, _queue_kind) = self.runtime.block_on(self.scheduler.pop());

                if let Some(ctrl_ann) = ev.as_control() {
                    match ctrl_ann {
                        fatal @ ControlAnnouncement::FatalError { .. } => {
                            panic!(
                                "a control announcement requesting a fatal error was received: {}",
                                fatal
                            )
                        }
                    }
                } else {
                    debug!(?ev, "ignoring event while looking for a fatal")
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

impl<REv: 'static> Default for ComponentHarness<REv> {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// A special event for unit tests.
///
/// Essentially discards all event (they are not even processed by the unit testing hardness),
/// except for control announcements, which are preserved.
#[derive(Debug, From)]
pub enum UnitTestEvent {
    /// A preserved control announcement.
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    /// A different event.
    Other,
}

impl ReactorEvent for UnitTestEvent {
    fn as_control(&self) -> Option<&ControlAnnouncement> {
        match self {
            UnitTestEvent::ControlAnnouncement(ctrl_ann) => Some(ctrl_ann),
            UnitTestEvent::Other => None,
        }
    }
}

#[test]
fn default_works_without_panicking_for_component_harness() {
    let _harness = ComponentHarness::<()>::default();
}
