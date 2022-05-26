//! Effects subsystem.
//!
//! Effects describe things that the creator of the effect intends to happen, producing a value upon
//! completion (they actually are boxed futures).
//!
//! A pinned, boxed future returning an event is called an effect and typed as an `Effect<Ev>`,
//! where `Ev` is the event's type, as every effect must have its return value either wrapped in an
//! event through [`EffectExt::event`](crate::effect::EffectExt::event) or ignored using
//! [`EffectExt::ignore`](crate::effect::EffectExt::ignore). As an example, the
//! [`handle_event`](crate::components::Component::handle_event) function of a component always
//! returns `Effect<Self::Event>`.
//!
//! # A primer on events
//!
//! There are three distinct groups of events found around the node:
//!
//! * (unbound) events: These events are not associated with a particular reactor or component and
//!   represent information or requests by themselves. An example is the
//!   [`BlocklistAnnouncement`](`crate::effect::announcements::BlocklistAnnouncement`), it can be
//!   emitted through an effect by different components and contains the ID of a peer that should be
//!   shunned. It is not associated with a particular reactor or component though.
//!
//!   While the node is running, these unbound events cannot exist on their own, instead they are
//!   typically converted into a concrete reactor event by the effect builder as soon as they are
//!   created.
//!
//! * reactor events: A running reactor has a single event type that encompasses all possible
//!   unbound events that can occur during its operation and all component events of components it
//!   is made of. Usually they are implemented as one large `enum` with only newtype-variants.
//!
//! * component events: Every component defines its own set of events, typically for internal use.
//!   If the component is able to process unbound events like announcements or requests, it will
//!   have a `From` implementation that allows converting them into a suitable component event.
//!
//!   Component events are also created from the return values of effects: While effects do not
//!   return events themselves when called, their return values are turned first into component
//!   events through the [`event`](crate::effect::EffectExt) method. In a second step, inside the
//!   reactors routing code, `wrap_effect` will then convert from component to reactor event.
//!
//! # Using effects
//!
//! To create an effect, an `EffectBuilder` will be passed in by the calling reactor runner. For
//! example, given an effect builder `effect_builder`, we can create a `set_timeout` future and turn
//! it into an effect:
//!
//! ```ignore
//! use std::time::Duration;
//! use casper_node::effect::EffectExt;
//!
//! // Note: This is our "component" event.
//! enum Event {
//!     ThreeSecondsElapsed(Duration)
//! }
//!
//! effect_builder
//!     .set_timeout(Duration::from_secs(3))
//!     .event(Event::ThreeSecondsElapsed);
//! ```
//!
//! This example will produce an effect that, after three seconds, creates an
//! `Event::ThreeSecondsElapsed`. Note that effects do nothing on their own, they need to be passed
//! to a [`reactor`](../reactor/index.html) to be executed.
//!
//! # Arbitrary effects
//!
//! While it is technically possible to turn any future into an effect, it is in general advisable
//! to only use the methods on [`EffectBuilder`] or short, anonymous futures to create effects.
//!
//! # Announcements and requests
//!
//! Events are usually classified into either announcements or requests, although these properties
//! are not reflected in the type system.
//!
//! **Announcements** are events that are essentially "fire-and-forget"; the component that created
//! the effect resulting in the creation of the announcement will never expect an "answer".
//! Announcements are often dispatched to multiple components by the reactor; since that usually
//! involves a [`clone`](`Clone::clone`), they should be kept light.
//!
//! A good example is the arrival of a new deploy passed in by a client. Depending on the setup it
//! may be stored, buffered or, in certain testing setups, just discarded. None of this is a concern
//! of the component that talks to the client and deserializes the incoming deploy though, instead
//! it simply returns an effect that produces an announcement.
//!
//! **Requests** are complex events that are used when a component needs something from other
//! components. Typically, an effect (which uses [`EffectBuilder::make_request`] in its
//! implementation) is called resulting in the actual request being scheduled and handled. In
//! contrast to announcements, requests must always be handled by exactly one component.
//!
//! Every request has a [`Responder`]-typed field, which a handler of a request calls to produce
//! another effect that will send the return value to the original requesting component. Failing to
//! call the [`Responder::respond`] function will result in a runtime warning.

pub(crate) mod announcements;
pub(crate) mod diagnostics_port;
pub(crate) mod incoming;
pub(crate) mod requests;

use std::{
    any::type_name,
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    future::Future,
    mem,
    sync::Arc,
    time::{Duration, Instant},
};

use datasize::DataSize;
use futures::{channel::oneshot, future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize, Serializer};
use smallvec::{smallvec, SmallVec};
use tokio::{sync::Semaphore, time};
use tracing::{debug, error, warn};

use casper_execution_engine::{
    core::engine_state::{
        self, era_validators::GetEraValidatorsError, genesis::GenesisSuccess, BalanceRequest,
        BalanceResult, GetBidsRequest, GetBidsResult, QueryRequest, QueryResult, UpgradeConfig,
        UpgradeSuccess,
    },
    shared::execution_journal::ExecutionJournal,
    storage::trie::{TrieOrChunk, TrieOrChunkId},
};
use casper_hashing::Digest;
use casper_types::{
    account::Account, bytesrepr::Bytes, system::auction::EraValidators, Contract, ContractPackage,
    EraId, ExecutionEffect, ExecutionResult, Key, ProtocolVersion, PublicKey, TimeDiff, Timestamp,
    Transfer, URef, U512,
};

use crate::{
    components::{
        block_validator::ValidatingBlock,
        chainspec_loader::{CurrentRunInfo, NextUpgrade},
        consensus::{BlockContext, ClContext, EraDump, ValidatorChange},
        contract_runtime::{
            BlockAndExecutionEffects, BlockExecutionError, EraValidatorsRequest, ExecutionPreState,
        },
        deploy_acceptor,
        fetcher::FetchResult,
        small_network::FromIncoming,
    },
    reactor::{EventQueueHandle, QueueKind},
    types::{
        AvailableBlockRange, Block, BlockAndDeploys, BlockHash, BlockHeader,
        BlockHeaderWithMetadata, BlockPayload, BlockSignatures, BlockWithMetadata, Chainspec,
        ChainspecInfo, ChainspecRawBytes, Deploy, DeployHash, DeployHeader, DeployMetadataExt,
        DeployWithFinalizedApprovals, FinalitySignature, FinalizedApprovals, FinalizedBlock, Item,
        NodeId,
    },
    utils::{SharedFlag, Source},
};
use announcements::{
    BlockProposerAnnouncement, BlocklistAnnouncement, ChainspecLoaderAnnouncement,
    ConsensusAnnouncement, ContractRuntimeAnnouncement, ControlAnnouncement,
    DeployAcceptorAnnouncement, GossiperAnnouncement, LinearChainAnnouncement, QueueDumpFormat,
    RpcServerAnnouncement,
};
use diagnostics_port::DumpConsensusStateRequest;
use requests::{
    BeginGossipRequest, BlockPayloadRequest, BlockProposerRequest, BlockValidationRequest,
    ChainspecLoaderRequest, ConsensusRequest, ContractRuntimeRequest, FetcherRequest,
    MetricsRequest, NetworkInfoRequest, NetworkRequest, StateStoreRequest, StorageRequest,
};

/// A resource that will never be available, thus trying to acquire it will wait forever.
static UNOBTAINABLE: Lazy<Semaphore> = Lazy::new(|| Semaphore::new(0));

/// A pinned, boxed future that produces one or more events.
pub(crate) type Effect<Ev> = BoxFuture<'static, Multiple<Ev>>;

/// Multiple effects in a container.
pub(crate) type Effects<Ev> = Multiple<Effect<Ev>>;

/// A small collection of rarely more than two items.
///
/// Stored in a `SmallVec` to avoid allocations in case there are less than three items grouped. The
/// size of two items is chosen because one item is the most common use case, and large items are
/// typically boxed. In the latter case two pointers and one enum variant discriminator is almost
/// the same size as an empty vec, which is two pointers.
pub(crate) type Multiple<T> = SmallVec<[T; 2]>;

/// A responder satisfying a request.
#[must_use]
#[derive(DataSize)]
pub(crate) struct Responder<T> {
    /// Sender through which the response ultimately should be sent.
    sender: Option<oneshot::Sender<T>>,
    /// Reactor flag indicating shutdown.
    is_shutting_down: SharedFlag,
}

/// A responder that will automatically send a `None` on drop.
#[must_use]
#[derive(DataSize, Debug)]
pub(crate) struct AutoClosingResponder<T>(Responder<Option<T>>);

impl<T> AutoClosingResponder<T> {
    /// Creates a new auto closing responder from a responder of `Option<T>`.
    pub(crate) fn from_opt_responder(responder: Responder<Option<T>>) -> Self {
        AutoClosingResponder(responder)
    }

    /// Extracts the inner responder.
    fn into_inner(mut self) -> Responder<Option<T>> {
        let is_shutting_down = self.0.is_shutting_down;
        mem::replace(
            &mut self.0,
            Responder {
                sender: None,
                is_shutting_down,
            },
        )
    }
}

impl<T: Debug> AutoClosingResponder<T> {
    /// Send `Some(data)` to the origin of the request.
    pub(crate) async fn respond(self, data: T) {
        self.into_inner().respond(Some(data)).await
    }

    /// Send `None` to the origin of the request.
    pub(crate) async fn respond_none(self) {
        self.into_inner().respond(None).await
    }
}

impl<T> Drop for AutoClosingResponder<T> {
    fn drop(&mut self) {
        if let Some(sender) = self.0.sender.take() {
            debug!(
                sending_value = %self.0,
                "responding None by dropping auto-close responder"
            );
            // We still haven't answered, send an answer.
            if let Err(_unsent_value) = sender.send(None) {
                debug!(
                    unsent_value = %self.0,
                    "failed to auto-close responder, ignoring"
                )
            }
        }
    }
}

impl<T: 'static + Send> Responder<T> {
    /// Creates a new `Responder`.
    #[inline]
    fn new(sender: oneshot::Sender<T>, is_shutting_down: SharedFlag) -> Self {
        Responder {
            sender: Some(sender),
            is_shutting_down,
        }
    }

    /// Helper method for tests.
    ///
    /// Allows creating a responder manually, without observing the shutdown flag. This function
    /// should not be used, unless you are writing alternative infrastructure, e.g. for tests.
    #[cfg(test)]
    #[inline]
    pub(crate) fn without_shutdown(sender: oneshot::Sender<T>) -> Self {
        Responder::new(sender, SharedFlag::global_shared())
    }
}

impl<T: Debug> Responder<T> {
    /// Send `data` to the origin of the request.
    pub(crate) async fn respond(mut self, data: T) {
        if let Some(sender) = self.sender.take() {
            if let Err(data) = sender.send(data) {
                // If we cannot send a response down the channel, it means the original requestor is
                // no longer interested in our response. This typically happens during shutdowns, or
                // in cases where an originating external request has been cancelled.

                debug!(
                    ?data,
                    "ignored failure to send response to request down oneshot channel"
                );
            }
        } else {
            error!(
                ?data,
                "tried to send a value down a responder channel, but it was already used"
            );
        }
    }
}

impl<T> Debug for Responder<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "Responder<{}>", type_name::<T>(),)
    }
}

impl<T> Display for Responder<T> {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "responder({})", type_name::<T>(),)
    }
}

impl<T> Drop for Responder<T> {
    fn drop(&mut self) {
        if self.sender.is_some() {
            if self.is_shutting_down.is_set() {
                debug!(
                    responder=?self,
                    "ignored dropping of responder during shutdown"
                );
            } else {
                // This is usually a very serious error, as another component will now be stuck.
                //
                // See the code `make_request` for more details.
                error!(
                    responder=?self,
                    "dropped without being responded to outside of shutdown"
                );
            }
        }
    }
}

impl<T> Serialize for Responder<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&format!("{:?}", self))
    }
}

impl<T> Serialize for AutoClosingResponder<T> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        self.0.serialize(serializer)
    }
}

/// Effect extension for futures, used to convert futures into actual effects.
pub(crate) trait EffectExt: Future + Send {
    /// Finalizes a future into an effect that returns a single event.
    ///
    /// The function `f` is used to translate the returned value from an effect into an event.
    fn event<U, F>(self, f: F) -> Effects<U>
    where
        F: FnOnce(Self::Output) -> U + 'static + Send,
        U: 'static,
        Self: Sized;

    /// Finalizes a future into an effect that returns an iterator of events.
    ///
    /// The function `f` is used to translate the returned value from an effect into an iterator of
    /// events.
    fn events<U, F, I>(self, f: F) -> Effects<U>
    where
        F: FnOnce(Self::Output) -> I + 'static + Send,
        U: 'static,
        I: Iterator<Item = U>,
        Self: Sized;

    /// Finalizes a future into an effect that runs but drops the result.
    fn ignore<Ev>(self) -> Effects<Ev>;
}

/// Effect extension for futures, used to convert futures returning a `Result` into two different
/// effects.
pub(crate) trait EffectResultExt {
    /// The type the future will return if `Ok`.
    type Value;
    /// The type the future will return if `Err`.
    type Error;

    /// Finalizes a future returning a `Result` into two different effects.
    ///
    /// The function `f_ok` is used to translate the returned value from an effect into an event,
    /// while the function `f_err` does the same for a potential error.
    fn result<U, F, G>(self, f_ok: F, f_err: G) -> Effects<U>
    where
        F: FnOnce(Self::Value) -> U + 'static + Send,
        G: FnOnce(Self::Error) -> U + 'static + Send,
        U: 'static;
}

/// Effect extension for futures, used to convert futures returning an `Option` into two different
/// effects.
pub(crate) trait EffectOptionExt {
    /// The type the future will return if `Some`.
    type Value;

    /// Finalizes a future returning an `Option` into two different effects.
    ///
    /// The function `f_some` is used to translate the returned value from an effect into an event,
    /// while the function `f_none` does the same for a returned `None`.
    fn map_or_else<U, F, G>(self, f_some: F, f_none: G) -> Effects<U>
    where
        F: FnOnce(Self::Value) -> U + 'static + Send,
        G: FnOnce() -> U + 'static + Send,
        U: 'static;

    /// Finalizes a future returning an `Option` into two different effects.
    ///
    /// The function `f` is used to translate the returned value from an effect into an event,
    /// In the case of `None`, empty vector of effects is returned.
    fn map_some<U, F>(self, f: F) -> Effects<U>
    where
        F: FnOnce(Self::Value) -> U + 'static + Send,
        U: 'static;
}

impl<T: ?Sized> EffectExt for T
where
    T: Future + Send + 'static + Sized,
{
    fn event<U, F>(self, f: F) -> Effects<U>
    where
        F: FnOnce(Self::Output) -> U + 'static + Send,
        U: 'static,
    {
        smallvec![self.map(f).map(|item| smallvec![item]).boxed()]
    }

    fn events<U, F, I>(self, f: F) -> Effects<U>
    where
        F: FnOnce(Self::Output) -> I + 'static + Send,
        U: 'static,
        I: Iterator<Item = U>,
    {
        smallvec![self.map(f).map(|iter| iter.collect()).boxed()]
    }

    fn ignore<Ev>(self) -> Effects<Ev> {
        smallvec![self.map(|_| Multiple::new()).boxed()]
    }
}

impl<T, V, E> EffectResultExt for T
where
    T: Future<Output = Result<V, E>> + Send + 'static + Sized,
    T: ?Sized,
{
    type Value = V;
    type Error = E;

    fn result<U, F, G>(self, f_ok: F, f_err: G) -> Effects<U>
    where
        F: FnOnce(V) -> U + 'static + Send,
        G: FnOnce(E) -> U + 'static + Send,
        U: 'static,
    {
        smallvec![self
            .map(|result| result.map_or_else(f_err, f_ok))
            .map(|item| smallvec![item])
            .boxed()]
    }
}

impl<T, V> EffectOptionExt for T
where
    T: Future<Output = Option<V>> + Send + 'static + Sized,
    T: ?Sized,
{
    type Value = V;

    fn map_or_else<U, F, G>(self, f_some: F, f_none: G) -> Effects<U>
    where
        F: FnOnce(V) -> U + 'static + Send,
        G: FnOnce() -> U + 'static + Send,
        U: 'static,
    {
        smallvec![self
            .map(|option| option.map_or_else(f_none, f_some))
            .map(|item| smallvec![item])
            .boxed()]
    }

    /// Finalizes a future returning an `Option`.
    ///
    /// The function `f` is used to translate the returned value from an effect into an event,
    /// In the case of `None`, empty vector is returned.
    fn map_some<U, F>(self, f: F) -> Effects<U>
    where
        F: FnOnce(Self::Value) -> U + 'static + Send,
        U: 'static,
    {
        smallvec![self
            .map(|option| option
                .map(|el| smallvec![f(el)])
                .unwrap_or_else(|| smallvec![]))
            .boxed()]
    }
}

/// A builder for [`Effect`](type.Effect.html)s.
///
/// Provides methods allowing the creation of effects which need to be scheduled on the reactor's
/// event queue, without giving direct access to this queue.
///
/// The `REv` type parameter indicates which reactor event effects created by this builder will
/// produce as side-effects.
#[derive(Debug)]
pub(crate) struct EffectBuilder<REv: 'static> {
    /// A handle to the referenced event queue.
    event_queue: EventQueueHandle<REv>,
}

// Implement `Clone` and `Copy` manually, as `derive` will make it depend on `REv` otherwise.
impl<REv> Clone for EffectBuilder<REv> {
    fn clone(&self) -> Self {
        EffectBuilder {
            event_queue: self.event_queue,
        }
    }
}

impl<REv> Copy for EffectBuilder<REv> {}

impl<REv> EffectBuilder<REv> {
    /// Creates a new effect builder.
    pub(crate) fn new(event_queue: EventQueueHandle<REv>) -> Self {
        EffectBuilder { event_queue }
    }

    /// Extract the event queue handle out of the effect builder.
    #[cfg(test)]
    pub(crate) fn into_inner(self) -> EventQueueHandle<REv> {
        self.event_queue
    }

    /// Performs a request.
    ///
    /// Given a request `Q`, that when completed will yield a result of `T`, produces a future that
    /// will
    ///
    /// 1. create an event to send the request to the respective component (thus `Q: Into<REv>`),
    /// 2. wait for a response and return it.
    ///
    /// This function is usually only used internally by effects implemented on the effects builder,
    /// but IO components may also make use of it.
    ///
    /// # Cancellation safety
    ///
    /// This future is cancellation safe: If it is dropped without being polled, it merely indicates
    /// the original requestor is not longer interested in the result, which will be discarded.
    pub(crate) async fn make_request<T, Q, F>(self, f: F, queue_kind: QueueKind) -> T
    where
        T: Send + 'static,
        Q: Into<REv>,
        F: FnOnce(Responder<T>) -> Q,
    {
        let (event, wait_future) = self.create_request_parts(f);

        // Schedule the request before awaiting the response.
        self.event_queue.schedule(event, queue_kind).await;
        wait_future.await
    }

    /// Creates the part necessary to make a request.
    ///
    /// A request usually consists of two parts: The request event that needs to be scheduled on the
    /// reactor queue and associated future that allows waiting for the response. This function
    /// creates both of them without processing or spawning either.
    ///
    /// Usually you will want to call the higher level `make_request` function.
    pub(crate) fn create_request_parts<T, Q, F>(self, f: F) -> (REv, impl Future<Output = T>)
    where
        T: Send + 'static,
        Q: Into<REv>,
        F: FnOnce(Responder<T>) -> Q,
    {
        // Prepare a channel.
        let (sender, receiver) = oneshot::channel();

        // Create response function.
        let responder = Responder::new(sender, self.event_queue.shutdown_flag());

        // Now inject the request event into the event loop.
        let request_event = f(responder).into();

        let fut = async move {
            match receiver.await {
                Ok(value) => value,
                Err(err) => {
                    // The channel should usually not be closed except during shutdowns, as it
                    // indicates a panic or disappearance of the remote that is
                    // supposed to process the request.
                    //
                    // If it does happen, we pretend nothing happened instead of crashing.
                    if self.event_queue.shutdown_flag().is_set() {
                        debug!(%err, channel=?type_name::<T>(), "ignoring closed channel due to shutdown")
                    } else {
                        error!(%err, channel=?type_name::<T>(), "request for channel closed, this may be a bug? \
                            check if a component is stuck from now on");
                    }

                    // We cannot produce any value to satisfy the request, so we just abandon this
                    // task by waiting on a resource we can never acquire.
                    let _ = UNOBTAINABLE.acquire().await;
                    panic!("should never obtain unobtainable semaphore");
                }
            }
        };

        (request_event, fut)
    }

    /// Run and end effect immediately.
    ///
    /// Can be used to trigger events from effects when combined with `.event`. Do not use this to
    /// "do nothing", as it will still cause a task to be spawned.
    #[inline(always)]
    #[allow(clippy::manual_async_fn)]
    pub(crate) fn immediately(self) -> impl Future<Output = ()> + Send {
        // Note: This function is implemented manually without `async` sugar because the `Send`
        // inference seems to not work in all cases otherwise.
        async {}
    }

    /// Reports a fatal error.  Normally called via the `crate::fatal!()` macro.
    ///
    /// Usually causes the node to cease operations quickly and exit/crash.
    //
    // Note: This function is implemented manually without `async` sugar because the `Send`
    // inference seems to not work in all cases otherwise.
    pub(crate) async fn fatal(self, file: &'static str, line: u32, msg: String)
    where
        REv: From<ControlAnnouncement>,
    {
        self.event_queue
            .schedule(
                ControlAnnouncement::FatalError { file, line, msg },
                QueueKind::Control,
            )
            .await
    }

    /// Sets a timeout.
    pub(crate) async fn set_timeout(self, timeout: Duration) -> Duration {
        let then = Instant::now();
        time::sleep(timeout).await;
        Instant::now() - then
    }

    /// Retrieve a snapshot of the nodes current metrics formatted as string.
    ///
    /// If an error occurred producing the metrics, `None` is returned.
    pub(crate) async fn get_metrics(self) -> Option<String>
    where
        REv: From<MetricsRequest>,
    {
        self.make_request(
            |responder| MetricsRequest::RenderNodeMetricsText { responder },
            QueueKind::Api,
        )
        .await
    }

    /// Sends a network message.
    ///
    /// The message is queued and sent, but no delivery guaranteed. Will return after the message
    /// has been buffered in the outgoing kernel buffer and thus is subject to backpressure.
    pub(crate) async fn send_message<P>(self, dest: NodeId, payload: P)
    where
        REv: From<NetworkRequest<P>>,
    {
        self.make_request(
            |responder| NetworkRequest::SendMessage {
                dest: Box::new(dest),
                payload: Box::new(payload),
                respond_after_queueing: false,
                auto_closing_responder: AutoClosingResponder::from_opt_responder(responder),
            },
            QueueKind::Network,
        )
        .await;
    }

    /// Enqueues a network message.
    ///
    /// The message is queued in "fire-and-forget" fashion, there is no guarantee that the peer
    /// will receive it. Returns as soon as the message is queued inside the networking component.
    pub(crate) async fn enqueue_message<P>(self, dest: NodeId, payload: P)
    where
        REv: From<NetworkRequest<P>>,
    {
        self.make_request(
            |responder| NetworkRequest::SendMessage {
                dest: Box::new(dest),
                payload: Box::new(payload),
                respond_after_queueing: true,
                auto_closing_responder: AutoClosingResponder::from_opt_responder(responder),
            },
            QueueKind::Network,
        )
        .await;
    }

    /// Broadcasts a network message.
    ///
    /// Broadcasts a network message to all peers connected at the time the message is sent.
    pub(crate) async fn broadcast_message<P>(self, payload: P)
    where
        REv: From<NetworkRequest<P>>,
    {
        self.make_request(
            |responder| NetworkRequest::Broadcast {
                payload: Box::new(payload),
                auto_closing_responder: AutoClosingResponder::from_opt_responder(responder),
            },
            QueueKind::Network,
        )
        .await;
    }

    /// Gossips a network message.
    ///
    /// A low-level "gossip" function, selects `count` randomly chosen nodes on the network,
    /// excluding the indicated ones, and sends each a copy of the message.
    ///
    /// Returns the IDs of the chosen nodes.
    pub(crate) async fn gossip_message<P>(
        self,
        payload: P,
        count: usize,
        exclude: HashSet<NodeId>,
    ) -> HashSet<NodeId>
    where
        REv: From<NetworkRequest<P>>,
        P: Send,
    {
        self.make_request(
            |responder| NetworkRequest::Gossip {
                payload: Box::new(payload),
                count,
                exclude,
                auto_closing_responder: AutoClosingResponder::from_opt_responder(responder),
            },
            QueueKind::Network,
        )
        .await
        .unwrap_or_default()
    }

    /// Gets a map of the current network peers to their socket addresses.
    pub(crate) async fn network_peers(self) -> BTreeMap<NodeId, String>
    where
        REv: From<NetworkInfoRequest>,
    {
        self.make_request(
            |responder| NetworkInfoRequest::Peers { responder },
            QueueKind::Api,
        )
        .await
    }

    /// Gets the current network peers in random order.
    pub async fn get_fully_connected_peers(self) -> Vec<NodeId>
    where
        REv: From<NetworkInfoRequest>,
    {
        self.make_request(
            |responder| NetworkInfoRequest::FullyConnectedPeers { responder },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the current network non-joiner peers in random order.
    pub async fn get_fully_connected_non_joiner_peers(self) -> Vec<NodeId>
    where
        REv: From<NetworkInfoRequest>,
    {
        self.make_request(
            |responder| NetworkInfoRequest::FullyConnectedNonJoinerPeers { responder },
            QueueKind::Regular,
        )
        .await
    }

    /// Announces which deploys have expired.
    pub(crate) async fn announce_expired_deploys(self, hashes: Vec<DeployHash>)
    where
        REv: From<BlockProposerAnnouncement>,
    {
        self.event_queue
            .schedule(
                BlockProposerAnnouncement::DeploysExpired(hashes),
                QueueKind::Regular,
            )
            .await;
    }

    /// Announces an incoming network message.
    pub(crate) async fn announce_incoming<P>(self, sender: NodeId, payload: P)
    where
        REv: FromIncoming<P>,
    {
        self.event_queue
            .schedule(
                <REv as FromIncoming<P>>::from_incoming(sender, payload),
                QueueKind::NetworkIncoming,
            )
            .await
    }

    /// Announces that a gossiper has received a new item, where the item's ID is the complete item.
    pub(crate) async fn announce_complete_item_received_via_gossip<T: Item>(self, item: T::Id)
    where
        REv: From<GossiperAnnouncement<T>>,
    {
        assert!(
            T::ID_IS_COMPLETE_ITEM,
            "{} must be an item where the ID _is_ the complete item",
            item
        );
        self.event_queue
            .schedule(
                GossiperAnnouncement::NewCompleteItem(item),
                QueueKind::Regular,
            )
            .await;
    }

    /// Announces that the HTTP API server has received a deploy.
    pub(crate) async fn announce_deploy_received(
        self,
        deploy: Box<Deploy>,
        responder: Option<Responder<Result<(), deploy_acceptor::Error>>>,
    ) where
        REv: From<RpcServerAnnouncement>,
    {
        self.event_queue
            .schedule(
                RpcServerAnnouncement::DeployReceived { deploy, responder },
                QueueKind::Api,
            )
            .await;
    }

    /// Announces that a deploy not previously stored has now been accepted and stored.
    pub(crate) fn announce_new_deploy_accepted(
        self,
        deploy: Box<Deploy>,
        source: Source,
    ) -> impl Future<Output = ()>
    where
        REv: From<DeployAcceptorAnnouncement>,
    {
        self.event_queue.schedule(
            DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source },
            QueueKind::Regular,
        )
    }

    /// Announces that we have finished gossiping the indicated item.
    pub(crate) async fn announce_finished_gossiping<T>(self, item_id: T::Id)
    where
        REv: From<GossiperAnnouncement<T>>,
        T: Item,
    {
        self.event_queue
            .schedule(
                GossiperAnnouncement::FinishedGossiping(item_id),
                QueueKind::Regular,
            )
            .await;
    }

    /// Announces that an invalid deploy has been received.
    pub(crate) fn announce_invalid_deploy(
        self,
        deploy: Box<Deploy>,
        source: Source,
    ) -> impl Future<Output = ()>
    where
        REv: From<DeployAcceptorAnnouncement>,
    {
        self.event_queue.schedule(
            DeployAcceptorAnnouncement::InvalidDeploy { deploy, source },
            QueueKind::Regular,
        )
    }

    /// Announces upgrade activation point read.
    pub(crate) async fn announce_upgrade_activation_point_read(self, next_upgrade: NextUpgrade)
    where
        REv: From<ChainspecLoaderAnnouncement>,
    {
        self.event_queue
            .schedule(
                ChainspecLoaderAnnouncement::UpgradeActivationPointRead(next_upgrade),
                QueueKind::Regular,
            )
            .await
    }

    /// Announces a committed Step success.
    pub(crate) async fn announce_commit_step_success(
        self,
        era_id: EraId,
        execution_journal: ExecutionJournal,
    ) where
        REv: From<ContractRuntimeAnnouncement>,
    {
        self.event_queue
            .schedule(
                ContractRuntimeAnnouncement::CommitStepSuccess {
                    era_id,
                    execution_effect: ExecutionEffect::from(&execution_journal),
                },
                QueueKind::Regular,
            )
            .await
    }

    /// Announces a new block has been created.
    pub(crate) async fn announce_new_linear_chain_block(
        self,
        block: Box<Block>,
        execution_results: Vec<(DeployHash, DeployHeader, ExecutionResult)>,
    ) where
        REv: From<ContractRuntimeAnnouncement>,
    {
        self.event_queue
            .schedule(
                ContractRuntimeAnnouncement::LinearChainBlock {
                    block,
                    execution_results,
                },
                QueueKind::Regular,
            )
            .await
    }

    /// Announces validators for upcoming era.
    pub(crate) async fn announce_upcoming_era_validators(
        self,
        era_that_is_ending: EraId,
        upcoming_era_validators: BTreeMap<EraId, BTreeMap<PublicKey, U512>>,
    ) where
        REv: From<ContractRuntimeAnnouncement>,
    {
        self.event_queue
            .schedule(
                ContractRuntimeAnnouncement::UpcomingEraValidators {
                    era_that_is_ending,
                    upcoming_era_validators,
                },
                QueueKind::Regular,
            )
            .await
    }

    /// Begins gossiping an item.
    pub(crate) async fn begin_gossip<T>(self, item_id: T::Id, source: Source)
    where
        T: Item,
        REv: From<BeginGossipRequest<T>>,
    {
        self.make_request(
            |responder| BeginGossipRequest {
                item_id,
                source,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Puts the given block into the linear block store.
    pub(crate) async fn put_block_to_storage(self, block: Box<Block>) -> bool
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::PutBlock { block, responder },
            QueueKind::Regular,
        )
        .await
    }

    /// Puts the given block and its deploys into the store.
    pub(crate) async fn put_block_and_deploys_to_storage(self, block: Box<BlockAndDeploys>)
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::PutBlockAndDeploys { block, responder },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the requested block from the linear block store.
    pub(crate) async fn get_block_from_storage(self, block_hash: BlockHash) -> Option<Block>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetBlock {
                block_hash,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the requested block and its deploys from the store.
    pub(crate) async fn get_block_and_deploys_from_storage(
        self,
        block_hash: BlockHash,
    ) -> Option<BlockAndDeploys>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetBlockAndDeploys {
                block_hash,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the requested block header from the linear block store.
    pub(crate) async fn get_block_header_from_storage(
        self,
        block_hash: BlockHash,
        only_from_available_block_range: bool,
    ) -> Option<BlockHeader>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetBlockHeader {
                block_hash,
                only_from_available_block_range,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    pub(crate) async fn get_block_header_at_height_from_storage(
        self,
        block_height: u64,
        only_from_available_block_range: bool,
    ) -> Option<BlockHeader>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetBlockHeaderByHeight {
                block_height,
                only_from_available_block_range,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Checks if a block header exists in storage
    pub(crate) async fn block_header_exists(self, block_height: u64) -> bool
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::CheckBlockHeaderExistence {
                block_height,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the requested signatures for a given block hash.
    pub(crate) async fn get_signatures_from_storage(
        self,
        block_hash: BlockHash,
    ) -> Option<BlockSignatures>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetBlockSignatures {
                block_hash,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Puts a block header to storage.
    pub(crate) async fn put_block_header_to_storage(self, block_header: Box<BlockHeader>) -> bool
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::PutBlockHeader {
                block_header,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Puts the requested finality signatures into storage.
    pub(crate) async fn put_signatures_to_storage(self, signatures: BlockSignatures) -> bool
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::PutBlockSignatures {
                signatures,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the requested block's transfers from storage.
    pub(crate) async fn get_block_transfers_from_storage(
        self,
        block_hash: BlockHash,
    ) -> Option<Vec<Transfer>>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetBlockTransfers {
                block_hash,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Requests the header of the block containing the given deploy.
    pub(crate) async fn get_block_header_for_deploy_from_storage(
        self,
        deploy_hash: DeployHash,
    ) -> Option<BlockHeader>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetBlockHeaderForDeploy {
                deploy_hash,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Requests the highest block.
    pub(crate) async fn get_highest_block_from_storage(self) -> Option<Block>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetHighestBlock { responder },
            QueueKind::Regular,
        )
        .await
    }

    /// Requests the highest block header.
    pub(crate) async fn get_highest_block_header_from_storage(self) -> Option<BlockHeader>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetHighestBlockHeader { responder },
            QueueKind::Regular,
        )
        .await
    }

    /// Requests the header of the switch block at the given era ID.
    pub(crate) async fn get_switch_block_header_at_era_id_from_storage(
        self,
        era_id: EraId,
    ) -> Option<BlockHeader>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetSwitchBlockHeaderAtEraId { era_id, responder },
            QueueKind::Regular,
        )
        .await
    }

    /// Updates the lowest available block height in storage.
    pub(crate) async fn update_lowest_available_block_height_in_storage(self, height: u64)
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::UpdateLowestAvailableBlockHeight { height, responder },
            QueueKind::Regular,
        )
        .await
    }

    /// Requests the height range of fully available blocks (not just block headers).
    pub(crate) async fn get_available_block_range_from_storage(self) -> AvailableBlockRange
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetAvailableBlockRange { responder },
            QueueKind::Regular,
        )
        .await
    }

    /// Get a trie or chunk by its ID.
    pub(crate) async fn get_trie(
        self,
        trie_or_chunk_id: TrieOrChunkId,
    ) -> Result<Option<TrieOrChunk>, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::GetTrie {
                trie_or_chunk_id,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Get a trie by its hash key.
    pub(crate) async fn get_trie_full(
        self,
        trie_key: Digest,
    ) -> Result<Option<Bytes>, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::GetTrieFull {
                trie_key,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Puts a trie into the trie store and asynchronously returns any missing descendant trie keys.
    pub(crate) async fn put_trie_and_find_missing_descendant_trie_keys(
        self,
        trie_bytes: Bytes,
    ) -> Result<Vec<Digest>, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::PutTrie {
                trie_bytes,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Asynchronously returns any missing descendant trie keys given an ancestor.
    pub(crate) async fn find_missing_descendant_trie_keys(
        self,
        trie_key: Digest,
    ) -> Result<Vec<Digest>, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::FindMissingDescendantTrieKeys {
                trie_key,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Puts the given deploy into the deploy store.
    pub(crate) async fn put_deploy_to_storage(self, deploy: Box<Deploy>) -> bool
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::PutDeploy { deploy, responder },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the requested deploys from the deploy store.
    ///
    /// Returns the "original" deploys, which are the first received by the node, along with a
    /// potentially different set of approvals used during execution of the recorded block.
    pub(crate) async fn get_deploys_from_storage(
        self,
        deploy_hashes: Vec<DeployHash>,
    ) -> SmallVec<[Option<DeployWithFinalizedApprovals>; 1]>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetDeploys {
                deploy_hashes,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Stores the given execution results for the deploys in the given block in the linear block
    /// store.
    pub(crate) async fn put_execution_results_to_storage(
        self,
        block_hash: BlockHash,
        execution_results: HashMap<DeployHash, ExecutionResult>,
    ) where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::PutExecutionResults {
                block_hash: Box::new(block_hash),
                execution_results,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the requested deploys from the deploy store.
    pub(crate) async fn get_deploy_and_metadata_from_storage(
        self,
        deploy_hash: DeployHash,
    ) -> Option<(DeployWithFinalizedApprovals, DeployMetadataExt)>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetDeployAndMetadata {
                deploy_hash,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the requested block and its finality signatures.
    pub(crate) async fn get_block_at_height_with_metadata_from_storage(
        self,
        block_height: u64,
        only_from_available_block_range: bool,
    ) -> Option<BlockWithMetadata>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetBlockAndMetadataByHeight {
                block_height,
                only_from_available_block_range,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets a block and sufficient finality signatures from storage.
    pub(crate) async fn get_block_and_sufficient_finality_signatures_by_height_from_storage(
        self,
        block_height: u64,
    ) -> Option<BlockWithMetadata>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetBlockAndSufficientFinalitySignaturesByHeight {
                block_height,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the requested block and its associated metadata.
    pub(crate) async fn get_block_header_and_sufficient_finality_signatures_by_height_from_storage(
        self,
        block_height: u64,
    ) -> Option<BlockHeaderWithMetadata>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetBlockHeaderAndSufficientFinalitySignaturesByHeight {
                block_height,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the requested block by hash with its associated metadata.
    pub(crate) async fn get_block_with_metadata_from_storage(
        self,
        block_hash: BlockHash,
        only_from_available_block_range: bool,
    ) -> Option<BlockWithMetadata>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetBlockAndMetadataByHash {
                block_hash,
                only_from_available_block_range,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the highest block with its associated metadata.
    pub(crate) async fn get_highest_block_with_metadata_from_storage(
        self,
    ) -> Option<BlockWithMetadata>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetHighestBlockWithMetadata { responder },
            QueueKind::Regular,
        )
        .await
    }

    /// Fetches an item from a fetcher.
    pub(crate) async fn fetch<T>(self, id: T::Id, peer: NodeId) -> FetchResult<T>
    where
        REv: From<FetcherRequest<T>>,
        T: Item + 'static,
    {
        self.make_request(
            |responder| FetcherRequest {
                id,
                peer,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Passes the timestamp of a future block for which deploys are to be proposed.
    pub(crate) async fn request_block_payload(
        self,
        context: BlockContext<ClContext>,
        next_finalized: u64,
        accusations: Vec<PublicKey>,
        random_bit: bool,
    ) -> Arc<BlockPayload>
    where
        REv: From<BlockProposerRequest>,
    {
        self.make_request(
            |responder| {
                BlockProposerRequest::RequestBlockPayload(BlockPayloadRequest {
                    context,
                    next_finalized,
                    accusations,
                    random_bit,
                    responder,
                })
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Executes a finalized block.
    pub(crate) async fn execute_finalized_block(
        self,
        protocol_version: ProtocolVersion,
        execution_pre_state: ExecutionPreState,
        finalized_block: FinalizedBlock,
        deploys: Vec<Deploy>,
        transfers: Vec<Deploy>,
    ) -> Result<BlockAndExecutionEffects, BlockExecutionError>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::ExecuteBlock {
                protocol_version,
                execution_pre_state,
                finalized_block,
                deploys,
                transfers,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Enqueues a finalized proto-block execution.
    ///
    /// # Arguments
    ///
    /// * `finalized_block` - a finalized proto-block to add to the execution queue.
    /// * `deploys` - a vector of deploys and transactions that match the hashes in the finalized
    ///   block, in that order.
    pub(crate) async fn enqueue_block_for_execution(
        self,
        finalized_block: FinalizedBlock,
        deploys: Vec<Deploy>,
        transfers: Vec<Deploy>,
    ) where
        REv: From<ContractRuntimeRequest>,
    {
        self.event_queue
            .schedule(
                ContractRuntimeRequest::EnqueueBlockForExecution {
                    finalized_block,
                    deploys,
                    transfers,
                },
                QueueKind::Regular,
            )
            .await
    }

    /// Checks whether the deploys included in the block exist on the network and the block is
    /// valid.
    pub(crate) async fn validate_block<T>(self, sender: NodeId, block: T) -> bool
    where
        REv: From<BlockValidationRequest>,
        T: Into<ValidatingBlock>,
    {
        self.make_request(
            |responder| BlockValidationRequest {
                block: block.into(),
                sender,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Announces that a block has been finalized.
    pub(crate) async fn announce_finalized_block(self, finalized_block: FinalizedBlock)
    where
        REv: From<ConsensusAnnouncement>,
    {
        self.event_queue
            .schedule(
                ConsensusAnnouncement::Finalized(Box::new(finalized_block)),
                QueueKind::Regular,
            )
            .await
    }

    /// Announces that a finality signature has been created.
    pub(crate) async fn announce_created_finality_signature(
        self,
        finality_signature: FinalitySignature,
    ) where
        REv: From<ConsensusAnnouncement>,
    {
        self.event_queue
            .schedule(
                ConsensusAnnouncement::CreatedFinalitySignature(Box::new(finality_signature)),
                QueueKind::Regular,
            )
            .await
    }

    /// An equivocation has been detected.
    pub(crate) async fn announce_fault_event(
        self,
        era_id: EraId,
        public_key: PublicKey,
        timestamp: Timestamp,
    ) where
        REv: From<ConsensusAnnouncement>,
    {
        self.event_queue
            .schedule(
                ConsensusAnnouncement::Fault {
                    era_id,
                    public_key: Box::new(public_key),
                    timestamp,
                },
                QueueKind::Regular,
            )
            .await
    }

    /// Announce the intent to disconnect from a specific peer, which consensus thinks is faulty.
    pub(crate) async fn announce_disconnect_from_peer(self, peer: NodeId)
    where
        REv: From<BlocklistAnnouncement>,
    {
        self.event_queue
            .schedule(
                BlocklistAnnouncement::OffenseCommitted(Box::new(peer)),
                QueueKind::Regular,
            )
            .await
    }

    /// The linear chain has stored a newly-created block.
    pub(crate) async fn announce_block_added(self, block: Box<Block>)
    where
        REv: From<LinearChainAnnouncement>,
    {
        self.event_queue
            .schedule(
                LinearChainAnnouncement::BlockAdded(block),
                QueueKind::Regular,
            )
            .await
    }

    /// The linear chain has stored a new finality signature.
    pub(crate) async fn announce_finality_signature(self, fs: Box<FinalitySignature>)
    where
        REv: From<LinearChainAnnouncement>,
    {
        self.event_queue
            .schedule(
                LinearChainAnnouncement::NewFinalitySignature(fs),
                QueueKind::Regular,
            )
            .await
    }

    /// Runs the genesis process on the contract runtime.
    pub(crate) async fn commit_genesis(
        self,
        chainspec: Arc<Chainspec>,
        chainspec_raw_bytes: Arc<ChainspecRawBytes>,
    ) -> Result<GenesisSuccess, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::CommitGenesis {
                chainspec,
                chainspec_raw_bytes,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Runs the upgrade process on the contract runtime.
    pub(crate) async fn upgrade_contract_runtime(
        self,
        upgrade_config: Box<UpgradeConfig>,
    ) -> Result<UpgradeSuccess, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::Upgrade {
                upgrade_config,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the requested chainspec info from the chainspec loader.
    pub(crate) async fn get_chainspec_info(self) -> ChainspecInfo
    where
        REv: From<ChainspecLoaderRequest> + Send,
    {
        self.make_request(ChainspecLoaderRequest::GetChainspecInfo, QueueKind::Regular)
            .await
    }

    /// Gets the information about the current run of the node software.
    pub(crate) async fn get_current_run_info(self) -> CurrentRunInfo
    where
        REv: From<ChainspecLoaderRequest>,
    {
        self.make_request(
            ChainspecLoaderRequest::GetCurrentRunInfo,
            QueueKind::Regular,
        )
        .await
    }

    /// Retrieves finalized blocks with timestamps no older than the maximum deploy TTL.
    ///
    /// These blocks contain all deploy and transfer hashes that are known to be finalized but
    /// may not have expired yet.
    pub(crate) async fn get_finalized_blocks(self, ttl: TimeDiff) -> Vec<Block>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            move |responder| StorageRequest::GetFinalizedBlocks { ttl, responder },
            QueueKind::Regular,
        )
        .await
    }

    /// Loads potentially previously stored state from storage.
    ///
    /// Key must be a unique key across the the application, as all keys share a common namespace.
    ///
    /// If an error occurs during state loading or no data is found, returns `None`.
    pub(crate) async fn load_state<T>(self, key: Cow<'static, [u8]>) -> Option<T>
    where
        REv: From<StateStoreRequest>,
        for<'de> T: Deserialize<'de>,
    {
        // Due to object safety issues, we cannot ship the actual values around, but only the
        // serialized bytes. Hence we retrieve raw bytes from storage and then deserialize here.
        self.make_request(
            move |responder| StateStoreRequest::Load { key, responder },
            QueueKind::Regular,
        )
        .await
        .map(|data| bincode::deserialize(&data))
        .transpose()
        .unwrap_or_else(|err| {
            let type_name = type_name::<T>();
            error!(%type_name, %err, "could not deserialize state from storage");
            None
        })
    }

    /// Save state to storage.
    ///
    /// Key must be a unique key across the the application, as all keys share a common namespace.
    ///
    /// Returns whether or not storing the state was successful. A component that requires state to
    /// be successfully stored should check the return value and act accordingly.
    pub(crate) async fn save_state<T>(self, key: Cow<'static, [u8]>, value: T) -> bool
    where
        REv: From<StateStoreRequest>,
        T: Serialize,
    {
        match bincode::serialize(&value) {
            Ok(data) => {
                self.make_request(
                    move |responder| StateStoreRequest::Save {
                        key,
                        data,
                        responder,
                    },
                    QueueKind::Regular,
                )
                .await;
                true
            }
            Err(err) => {
                let type_name = type_name::<T>();
                warn!(%type_name, %err, "error serializing state");
                false
            }
        }
    }

    /// Requests a query be executed on the Contract Runtime component.
    pub(crate) async fn query_global_state(
        self,
        query_request: QueryRequest,
    ) -> Result<QueryResult, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::Query {
                query_request,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Retrieves an `Account` from global state if present.
    pub(crate) async fn get_account_from_global_state(
        self,
        prestate_hash: Digest,
        account_key: Key,
    ) -> Option<Account>
    where
        REv: From<ContractRuntimeRequest>,
    {
        let query_request = QueryRequest::new(prestate_hash, account_key, vec![]);
        match self.query_global_state(query_request).await {
            Ok(QueryResult::Success { value, .. }) => value.as_account().cloned(),
            Ok(_) | Err(_) => None,
        }
    }

    /// Retrieves the balance of a purse, returns `None` if no purse is present.
    pub(crate) async fn check_purse_balance(
        self,
        prestate_hash: Digest,
        main_purse: URef,
    ) -> Option<U512>
    where
        REv: From<ContractRuntimeRequest>,
    {
        let balance_request = BalanceRequest::new(prestate_hash, main_purse);
        match self.get_balance(balance_request).await {
            Ok(balance_result) => {
                if let Some(motes) = balance_result.motes() {
                    return Some(*motes);
                }
                None
            }
            Err(_) => None,
        }
    }

    /// Retrieves an `Contract` from global state if present.
    pub(crate) async fn get_contract_for_validation(
        self,
        prestate_hash: Digest,
        query_key: Key,
        path: Vec<String>,
    ) -> Option<Contract>
    where
        REv: From<ContractRuntimeRequest>,
    {
        let query_request = QueryRequest::new(prestate_hash, query_key, path);
        match self.query_global_state(query_request).await {
            Ok(QueryResult::Success { value, .. }) => value.as_contract().cloned(),
            Ok(_) | Err(_) => None,
        }
    }

    /// Retrieves an `ContractPackage` from global state if present.
    pub(crate) async fn get_contract_package_for_validation(
        self,
        prestate_hash: Digest,
        query_key: Key,
        path: Vec<String>,
    ) -> Option<ContractPackage>
    where
        REv: From<ContractRuntimeRequest>,
    {
        let query_request = QueryRequest::new(prestate_hash, query_key, path);
        match self.query_global_state(query_request).await {
            Ok(QueryResult::Success { value, .. }) => value.as_contract_package().cloned(),
            Ok(_) | Err(_) => None,
        }
    }

    /// Requests a query be executed on the Contract Runtime component.
    pub(crate) async fn get_balance(
        self,
        balance_request: BalanceRequest,
    ) -> Result<BalanceResult, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::GetBalance {
                balance_request,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Returns a map of validators weights for all eras as known from `root_hash`.
    ///
    /// This operation is read only.
    pub(crate) async fn get_era_validators_from_contract_runtime(
        self,
        request: EraValidatorsRequest,
    ) -> Result<EraValidators, GetEraValidatorsError>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::GetEraValidators { request, responder },
            QueueKind::Regular,
        )
        .await
    }

    /// Requests a query be executed on the Contract Runtime component.
    pub(crate) async fn get_bids(
        self,
        get_bids_request: GetBidsRequest,
    ) -> Result<GetBidsResult, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::GetBids {
                get_bids_request,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the correct era validators set for the given era.
    /// Takes emergency restarts into account based on the information from the chainspec loader.
    pub(crate) async fn get_era_validators(self, era_id: EraId) -> Option<BTreeMap<PublicKey, U512>>
    where
        REv: From<StorageRequest> + From<ChainspecLoaderRequest>,
    {
        let CurrentRunInfo {
            last_emergency_restart,
            ..
        } = self.get_current_run_info().await;
        let cutoff_era_id = last_emergency_restart.unwrap_or_else(|| EraId::new(0));
        if era_id < cutoff_era_id {
            // we don't support getting the validators from before the last emergency restart
            return None;
        }
        // Era 0 contains no blocks other than the genesis immediate switch block which can be used
        // to get the validators for era 0.  For any other era `n`, we need the switch block from
        // era `n-1` to get the validators for `n`.
        let era_before = era_id.saturating_sub(1);
        self.get_switch_block_header_at_era_id_from_storage(era_before)
            .await
            .and_then(BlockHeader::maybe_take_next_era_validator_weights)
    }

    /// Checks whether the given validator is bonded in the given era.
    pub(crate) async fn is_bonded_validator(
        self,
        validator: PublicKey,
        era_id: EraId,
        latest_state_root_hash: Option<Digest>,
        protocol_version: ProtocolVersion,
    ) -> Result<bool, GetEraValidatorsError>
    where
        REv: From<ContractRuntimeRequest> + From<StorageRequest> + From<ChainspecLoaderRequest>,
    {
        // try just reading the era validators first
        let maybe_era_validators = self.get_era_validators(era_id).await;
        let maybe_is_currently_bonded =
            maybe_era_validators.map(|validators| validators.contains_key(&validator));

        match maybe_is_currently_bonded {
            // if we know whether the validator is bonded, just return that
            Some(is_bonded) => Ok(is_bonded),
            // if not, try checking future eras with the latest state root hash
            None => match latest_state_root_hash {
                // no root hash later than initial -> we just assume the validator is not bonded
                None => Ok(false),
                // otherwise, check with contract runtime
                Some(state_root_hash) => self
                    .make_request(
                        |responder| ContractRuntimeRequest::IsBonded {
                            state_root_hash,
                            era_id,
                            protocol_version,
                            public_key: validator,
                            responder,
                        },
                        QueueKind::Regular,
                    )
                    .await
                    .or_else(|error| {
                        // Promote this error to a non-error case.
                        // It's not an error that we can't find the era that was requested.
                        if error.is_era_validators_missing() {
                            Ok(false)
                        } else {
                            Err(error)
                        }
                    }),
            },
        }
    }

    /// Get our public key from consensus, and if we're a validator, the next round length.
    pub(crate) async fn consensus_status(self) -> Option<(PublicKey, Option<TimeDiff>)>
    where
        REv: From<ConsensusRequest>,
    {
        self.make_request(ConsensusRequest::Status, QueueKind::Regular)
            .await
    }

    /// Returns a list of validator status changes, by public key.
    pub(crate) async fn get_consensus_validator_changes(
        self,
    ) -> BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>>
    where
        REv: From<ConsensusRequest>,
    {
        self.make_request(ConsensusRequest::ValidatorChanges, QueueKind::Regular)
            .await
    }

    /// Dump consensus state for a specific era, using the supplied function to serialize the
    /// output.
    pub(crate) async fn diagnostics_port_dump_consensus_state(
        self,
        era_id: Option<EraId>,
        serialize: fn(&EraDump<'_>) -> Result<Vec<u8>, Cow<'static, str>>,
    ) -> Result<Vec<u8>, Cow<'static, str>>
    where
        REv: From<DumpConsensusStateRequest>,
    {
        self.make_request(
            |responder| DumpConsensusStateRequest {
                era_id,
                serialize,
                responder,
            },
            QueueKind::Control,
        )
        .await
    }

    /// Dump the event queue contents to the diagnostics port, using the given serializer.
    pub(crate) async fn diagnostics_port_dump_queue(self, dump_format: QueueDumpFormat)
    where
        REv: From<ControlAnnouncement>,
    {
        self.make_request(
            |responder| ControlAnnouncement::QueueDumpRequest {
                dump_format,
                finished: responder,
            },
            QueueKind::Control,
        )
        .await
    }

    /// Get the bytes for the chainspec file and genesis_accounts
    /// and global_state bytes if the files are present.
    pub(crate) async fn get_chainspec_raw_bytes(self) -> Arc<ChainspecRawBytes>
    where
        REv: From<ChainspecLoaderRequest> + Send,
    {
        self.make_request(
            ChainspecLoaderRequest::GetChainspecRawBytes,
            QueueKind::Regular,
        )
        .await
    }

    /// Stores a set of given finalized approvals in storage.
    ///
    /// Any previously stored finalized approvals for the given hash are quietly overwritten
    pub(crate) async fn store_finalized_approvals(
        self,
        deploy_hash: DeployHash,
        finalized_approvals: FinalizedApprovals,
    ) where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::StoreFinalizedApprovals {
                deploy_hash,
                finalized_approvals,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }
}

/// Construct a fatal error effect.
///
/// This macro is a convenient wrapper around `EffectBuilder::fatal` that inserts the `file!()` and
/// `line!()` number automatically.
#[macro_export]
macro_rules! fatal {
    ($effect_builder:expr, $($arg:tt)*) => {
        $effect_builder.fatal(file!(), line!(), format!($($arg)*))
    };
}
