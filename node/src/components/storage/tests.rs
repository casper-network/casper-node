use std::any::type_name;

use futures::channel::oneshot;
use tempfile::TempDir;
use tokio::runtime::{self, Runtime};

use super::{Config, Storage};
use crate::{
    components::Component,
    crypto::hash::Digest,
    effect::{requests::StorageRequest, EffectBuilder, Effects, Responder},
    reactor::{EventQueueHandle, QueueKind, Scheduler},
    testing::TestRng,
    types::BlockHash,
    utils::WithDir,
};

/// Storage component test fixture.
///
/// Creates a storage component in a temporary directory.
///
/// # Panics
///
/// Panics if setting up the storage fixture fails.
fn storage_fixture(harness: &mut ComponentHarness<()>) -> Storage {
    let (cfg, tmp) = Config::default_for_tests();

    Storage::new(&WithDir::new("/", cfg)).expect(
        "could not create storage component
    fixture",
    )
}

/// Environment to test a single component as isolated as possible.
///
/// Contains enough reactor machinery to drive a single component and a temporary directory.
struct ComponentHarness<REv: 'static> {
    /// Test random number generator instance.
    rng: TestRng,
    /// Scheduler for events. Only explicitly polled by the harness.
    scheduler: &'static Scheduler<REv>,
    /// An event queue handle to the scheduler.
    event_queue_handle: EventQueueHandle<REv>,
    /// Effect builder pointing at the scheduler.
    effect_builder: EffectBuilder<REv>,
    /// A temporary directy that can be used to store various data.
    tmp: TempDir,
    /// The `async` runtime used to execute effects.
    runtime: Runtime,
}

impl<REv: 'static> ComponentHarness<REv> {
    /// Creates a new component test harness.
    ///
    /// # Panics
    ///
    /// Panics if a temporary directory cannot be created.
    fn new() -> Self {
        let rng = TestRng::new();
        let scheduler = Box::leak(Box::new(Scheduler::new(QueueKind::weights())));
        let event_queue_handle = EventQueueHandle::new(scheduler);
        let effect_builder = EffectBuilder::new(event_queue_handle);

        ComponentHarness {
            rng,
            scheduler,
            event_queue_handle,
            effect_builder,
            tmp: TempDir::new().expect("could not create temporary directory for test harness"),
            runtime: runtime::Builder::new()
                .enable_all()
                .build()
                .expect("could not build tokio runtime"),
        }
    }

    /// Send a request, expecting an immediate response.
    ///
    /// Sends a request by creating a channel for the response, then mapping it using the function
    /// `f`. Executes all returned effects, then awaits a response.
    fn send_request<C, T, F>(&mut self, component: &mut C, f: F) -> T
    where
        C: Component<REv>,
        <C as Component<REv>>::Event: Send + 'static,
        T: Send + 'static,
        F: FnOnce(Responder<T>) -> C::Event,
    {
        // Prepare a channel.
        let (sender, receiver) = oneshot::channel();

        // Create response function.
        let responder = Responder::create(sender);

        // Create the event for the component.
        let request_event = f(responder).into();

        // Send directly to component.
        let returned_effects = self.send_event(component, request_event);

        // Execute the effects on our dedicated runtime, hopefully creating the responses.
        for effect in returned_effects {
            self.runtime.spawn(effect);
        }

        // Wait for a response to arrive.
        self.runtime.block_on(receiver).unwrap_or_else(|err| {
            // The channel should never be closed, ever.
            panic!(
                "request for {} channel closed, this is a serious bug --- \
                   a component will likely be stuck from now on ",
                type_name::<T>()
            );
        })
    }

    #[inline]
    fn send_event<C>(&mut self, component: &mut C, ev: C::Event) -> Effects<C::Event>
    where
        C: Component<REv>,
    {
        component.handle_event(self.effect_builder, &mut self.rng, ev)
    }
}

#[test]
fn get_block_of_non_existant_block_returns_none() {
    let mut harness = ComponentHarness::new();
    let mut storage = storage_fixture(&mut harness);

    let (sender, receiver) = oneshot::channel();
    let responder = Responder::create(sender);

    let req = StorageRequest::GetBlock {
        block_hash: BlockHash::new(Digest::random(&mut harness.rng)),
        responder,
    };

    storage.handle_event(harness.effect_builder, &mut harness.rng, req.into());
}

// #[test]
// fn can_put_and_get_block() {
//     panic!("YAY");
// }

// fn STORAGE(s: StorageRequest) {
//     match s {
//         StorageRequest::PutBlock { block, responder } => {}
//         StorageRequest::GetBock { block_hash, responder } => {}
//         StorageRequest::GetBlockAtHeight { height, responder } => {}
//         StorageRequest::GetHighestBlock { responder } => {}
//         StorageRequest::GetBlockHeader { block_hash, responder } => {}
//         StorageRequest::PutDeploy { deploy, responder } => {}
//         StorageRequest::GetDeploys { deploy_hashes, responder } => {}
//         StorageRequest::GetDeployHeaders { deploy_hashes, responder } => {}
//         StorageRequest::PutExecutionResults { block_hash, execution_results, responder } => {}
//         StorageRequest::GetDeployAndMetadata { deploy_hash, responder } => {}
//         StorageRequest::PutChainspec { chainspec, responder } => {}
//         StorageRequest::GetChainspec { version, responder } => {}
//     }
// }
