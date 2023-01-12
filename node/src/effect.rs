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
//!   [`PeerBehaviorAnnouncement`](`crate::effect::announcements::PeerBehaviorAnnouncement`), it can
//!   be emitted through an effect by different components and contains the ID of a peer that should
//!   be shunned. It is not associated with a particular reactor or component though.
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

use casper_storage::global_state::storage::trie::TrieRaw;
use datasize::DataSize;
use futures::{channel::oneshot, future::BoxFuture, FutureExt};
use once_cell::sync::Lazy;
use serde::{Serialize, Serializer};
use smallvec::{smallvec, SmallVec};
use tokio::{sync::Semaphore, time};
use tracing::{debug, error, warn};

use casper_execution_engine::{
    core::engine_state::{
        self, era_validators::GetEraValidatorsError, BalanceRequest, BalanceResult, GetBidsRequest,
        GetBidsResult, QueryRequest, QueryResult,
    },
    shared::execution_journal::ExecutionJournal,
};
use casper_hashing::Digest;
use casper_types::{
    account::Account, bytesrepr::Bytes, system::auction::EraValidators, Contract, ContractPackage,
    EraId, ExecutionEffect, ExecutionResult, Key, PublicKey, TimeDiff, Timestamp, Transfer, URef,
    U512,
};

use crate::{
    components::{
        block_synchronizer::{
            BlockSynchronizerStatus, GlobalStateSynchronizerError, TrieAccumulatorError,
        },
        consensus::{ClContext, EraDump, ProposedBlock, ValidatorChange},
        contract_runtime::{ContractRuntimeError, EraValidatorsRequest},
        deploy_acceptor,
        diagnostics_port::StopAtSpec,
        fetcher::FetchResult,
        network::{blocklist::BlocklistJustification, FromIncoming, NetworkInsights},
        upgrade_watcher::NextUpgrade,
    },
    contract_runtime::SpeculativeExecutionState,
    effect::{announcements::BlockSynchronizerAnnouncement, requests::BlockAccumulatorRequest},
    reactor::{main_reactor::ReactorState, EventQueueHandle, QueueKind},
    types::{
        appendable_block::AppendableBlock, ApprovalsHashes, AvailableBlockRange, Block,
        BlockExecutionResultsOrChunk, BlockExecutionResultsOrChunkId, BlockHash, BlockHeader,
        BlockSignatures, BlockWithMetadata, ChainspecRawBytes, Deploy, DeployHash, DeployHeader,
        DeployId, DeployMetadataExt, DeployWithFinalizedApprovals, FetcherItem, FinalitySignature,
        FinalitySignatureId, FinalizedApprovals, FinalizedBlock, GossiperItem, LegacyDeploy,
        NodeId, TrieOrChunk, TrieOrChunkId,
    },
    utils::{fmt_limit::FmtLimit, SharedFlag, Source},
};
use announcements::{
    BlockAccumulatorAnnouncement, ConsensusAnnouncement, ContractRuntimeAnnouncement,
    ControlAnnouncement, DeployAcceptorAnnouncement, DeployBufferAnnouncement, FatalAnnouncement,
    GossiperAnnouncement, PeerBehaviorAnnouncement, QueueDumpFormat, RpcServerAnnouncement,
    UpgradeWatcherAnnouncement,
};
use diagnostics_port::DumpConsensusStateRequest;
use requests::{
    BeginGossipRequest, BlockCompleteConfirmationRequest, BlockSynchronizerRequest,
    BlockValidationRequest, ChainspecRawBytesRequest, ConsensusRequest, FetcherRequest,
    MakeBlockExecutableRequest, NetworkInfoRequest, NetworkRequest, ReactorStatusRequest,
    StorageRequest, SyncGlobalStateRequest, TrieAccumulatorRequest, UpgradeWatcherRequest,
};

use self::requests::{
    ContractRuntimeRequest, DeployBufferRequest, MetricsRequest, SetNodeStopRequest,
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

/// The type of peers that should receive the gossip message.
#[derive(Debug, Serialize, PartialEq, Eq, Hash, Copy, Clone, DataSize)]
pub(crate) enum GossipTarget {
    /// Peers which are not validators in the given era.
    NonValidators(EraId),
    /// All peers.
    All,
}

impl Default for GossipTarget {
    fn default() -> Self {
        GossipTarget::All
    }
}

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
                // If we cannot send a response down the channel, it means the original requester is
                // no longer interested in our response. This typically happens during shutdowns, or
                // in cases where an originating external request has been cancelled.

                debug!(
                    data=?FmtLimit::new(1000, &data),
                    "ignored failure to send response to request down oneshot channel"
                );
            }
        } else {
            error!(
                data=?FmtLimit::new(1000, &data),
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
    /// the original requester is not longer interested in the result, which will be discarded.
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
    pub(crate) async fn fatal(self, file: &'static str, line: u32, msg: String)
    where
        REv: From<FatalAnnouncement>,
    {
        self.event_queue
            .schedule(FatalAnnouncement { file, line, msg }, QueueKind::Control)
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

    /// Broadcasts a network message to validator peers in the given era.
    pub(crate) async fn broadcast_message_to_validators<P>(self, payload: P, era_id: EraId)
    where
        REv: From<NetworkRequest<P>>,
    {
        self.make_request(
            |responder| {
                debug!("validator broadcast for {}", era_id);
                NetworkRequest::ValidatorBroadcast {
                    payload: Box::new(payload),
                    era_id,
                    auto_closing_responder: AutoClosingResponder::from_opt_responder(responder),
                }
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
        gossip_target: GossipTarget,
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
                gossip_target,
                count,
                exclude,
                auto_closing_responder: AutoClosingResponder::from_opt_responder(responder),
            },
            QueueKind::Network,
        )
        .await
        .unwrap_or_default()
    }

    /// Gets a structure describing the current network status.
    pub(crate) async fn get_network_insights(self) -> NetworkInsights
    where
        REv: From<NetworkInfoRequest>,
    {
        self.make_request(
            |responder| NetworkInfoRequest::Insight { responder },
            QueueKind::Regular,
        )
        .await
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

    /// Gets up to `count` fully-connected network peers in random order.
    pub async fn get_fully_connected_peers(self, count: usize) -> Vec<NodeId>
    where
        REv: From<NetworkInfoRequest>,
    {
        self.make_request(
            |responder| NetworkInfoRequest::FullyConnectedPeers { count, responder },
            QueueKind::NetworkInfo,
        )
        .await
    }

    /// Announces which deploys have expired.
    pub(crate) async fn announce_expired_deploys(self, hashes: Vec<DeployHash>)
    where
        REv: From<DeployBufferAnnouncement>,
    {
        self.event_queue
            .schedule(
                DeployBufferAnnouncement::DeploysExpired(hashes),
                QueueKind::Validation,
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
    pub(crate) async fn announce_complete_item_received_via_gossip<T: GossiperItem>(
        self,
        item: T::Id,
    ) where
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
                QueueKind::Gossip,
            )
            .await;
    }

    /// Announces that a gossiper has received a full item, where the item's ID is NOT the complete
    /// item.
    pub(crate) async fn announce_item_body_received_via_gossip<T: GossiperItem>(
        self,
        item: Box<T>,
        sender: NodeId,
    ) where
        REv: From<GossiperAnnouncement<T>>,
    {
        self.event_queue
            .schedule(
                GossiperAnnouncement::NewItemBody { item, sender },
                QueueKind::Gossip,
            )
            .await;
    }

    /// The block synchronizer has ensured that all the parts of this block are stored
    pub(crate) async fn announce_completed_block(self, block: Box<Block>)
    where
        REv: From<BlockSynchronizerAnnouncement>,
    {
        self.event_queue
            .schedule(
                BlockSynchronizerAnnouncement::CompletedBlock { block },
                QueueKind::Validation,
            )
            .await;
    }

    /// Announces that the block accumulator has received and stored a new block.
    pub(crate) async fn announce_block_added(self, block: Box<Block>)
    where
        REv: From<BlockAccumulatorAnnouncement>,
    {
        self.event_queue
            .schedule(
                BlockAccumulatorAnnouncement::AcceptedNewBlock { block },
                QueueKind::Validation,
            )
            .await;
    }

    /// Announces that the block accumulator has received and stored a new finality signature.
    pub(crate) async fn announce_finality_signature_accepted(
        self,
        finality_signature: Box<FinalitySignature>,
    ) where
        REv: From<BlockAccumulatorAnnouncement>,
    {
        self.event_queue
            .schedule(
                BlockAccumulatorAnnouncement::AcceptedNewFinalitySignature { finality_signature },
                QueueKind::FinalitySignature,
            )
            .await;
    }

    /// Request that a block be made executable (i.e. produce a FinalizedBlock plus any Deploys),
    /// if able to.
    ///
    /// Completion means that the block can be enqueued for processing by the execution engine via
    /// the contract_runtime component.
    pub(crate) async fn make_block_executable(
        self,
        block_hash: BlockHash,
    ) -> Option<(FinalizedBlock, Vec<Deploy>)>
    where
        REv: From<MakeBlockExecutableRequest>,
    {
        self.make_request(
            |responder| MakeBlockExecutableRequest {
                block_hash,
                responder,
            },
            QueueKind::FromStorage,
        )
        .await
    }

    /// Request that a block with a specific height be marked completed.
    ///
    /// Completion means that the block itself (along with its header) and all of its deploys have
    /// been persisted to storage and its global state root hash is missing no dependencies in the
    /// global state.
    pub(crate) async fn mark_block_completed(self, block_height: u64)
    where
        REv: From<BlockCompleteConfirmationRequest>,
    {
        self.make_request(
            |responder| BlockCompleteConfirmationRequest {
                block_height,
                responder,
            },
            QueueKind::FromStorage,
        )
        .await
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
            QueueKind::Validation,
        )
    }

    /// Announces that we have received a gossip message from this peer,
    /// implying the peer holds the indicated item.
    pub(crate) async fn announce_gossip_received<T>(self, item_id: T::Id, sender: NodeId)
    where
        REv: From<GossiperAnnouncement<T>>,
        T: GossiperItem,
    {
        self.event_queue
            .schedule(
                GossiperAnnouncement::GossipReceived { item_id, sender },
                QueueKind::Gossip,
            )
            .await;
    }

    /// Announces that we have finished gossiping the indicated item.
    pub(crate) async fn announce_finished_gossiping<T>(self, item_id: T::Id)
    where
        REv: From<GossiperAnnouncement<T>>,
        T: GossiperItem,
    {
        self.event_queue
            .schedule(
                GossiperAnnouncement::FinishedGossiping(item_id),
                QueueKind::Gossip,
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
            QueueKind::Validation,
        )
    }

    /// Announces upgrade activation point read.
    pub(crate) async fn announce_upgrade_activation_point_read(self, next_upgrade: NextUpgrade)
    where
        REv: From<UpgradeWatcherAnnouncement>,
    {
        self.event_queue
            .schedule(
                UpgradeWatcherAnnouncement::UpgradeActivationPointRead(next_upgrade),
                QueueKind::Control,
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
                QueueKind::ContractRuntime,
            )
            .await
    }

    /// Announces a new block has been created.
    pub(crate) async fn announce_executed_block(
        self,
        block: Box<Block>,
        approvals_hashes: Box<ApprovalsHashes>,
        execution_results: Vec<(DeployHash, DeployHeader, ExecutionResult)>,
    ) where
        REv: From<ContractRuntimeAnnouncement>,
    {
        self.event_queue
            .schedule(
                ContractRuntimeAnnouncement::ExecutedBlock {
                    block,
                    approvals_hashes,
                    execution_results,
                },
                QueueKind::ContractRuntime,
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
                QueueKind::ContractRuntime,
            )
            .await
    }

    /// Begins gossiping an item.
    pub(crate) async fn begin_gossip<T>(self, item_id: T::Id, source: Source)
    where
        T: GossiperItem,
        REv: From<BeginGossipRequest<T>>,
    {
        self.make_request(
            |responder| BeginGossipRequest {
                item_id,
                source,
                responder,
            },
            QueueKind::Gossip,
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
            QueueKind::ToStorage,
        )
        .await
    }

    /// Puts the given complete block into the linear block store.
    pub(crate) async fn put_complete_block_to_storage(self, block: Box<Block>) -> bool
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::PutCompleteBlock { block, responder },
            QueueKind::ToStorage,
        )
        .await
    }

    /// Puts the given approvals hashes into the linear block store.
    pub(crate) async fn put_approvals_hashes_to_storage(
        self,
        approvals_hashes: Box<ApprovalsHashes>,
    ) -> bool
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::PutApprovalsHashes {
                approvals_hashes,
                responder,
            },
            QueueKind::ToStorage,
        )
        .await
    }

    /// Puts the given block and approvals hashes into the linear block store.
    pub(crate) async fn put_executed_block_to_storage(
        self,
        block: Box<Block>,
        approvals_hashes: Box<ApprovalsHashes>,
        execution_results: HashMap<DeployHash, ExecutionResult>,
    ) -> bool
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::PutExecutedBlock {
                block,
                approvals_hashes,
                execution_results,
                responder,
            },
            QueueKind::ToStorage,
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
            QueueKind::FromStorage,
        )
        .await
    }

    /// Gets the requested `ApprovalsHashes` from storage.
    pub(crate) async fn get_approvals_hashes_from_storage(
        self,
        block_hash: BlockHash,
    ) -> Option<ApprovalsHashes>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetApprovalsHashes {
                block_hash,
                responder,
            },
            QueueKind::FromStorage,
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
            QueueKind::FromStorage,
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
            QueueKind::FromStorage,
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
            QueueKind::FromStorage,
        )
        .await
    }

    /// Gets the requested signature for a given block hash.
    pub(crate) async fn get_signature_from_storage(
        self,
        block_hash: BlockHash,
        public_key: PublicKey,
    ) -> Option<FinalitySignature>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetBlockSignature {
                block_hash,
                public_key: Box::new(public_key),
                responder,
            },
            QueueKind::FromStorage,
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
            QueueKind::ToStorage,
        )
        .await
    }

    /// Puts the requested block signatures into storage.
    ///
    /// If `signatures.proofs` is empty, no attempt to store will be made, an error will be logged,
    /// and this function will return `false`.
    pub(crate) async fn put_signatures_to_storage(self, signatures: BlockSignatures) -> bool
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::PutBlockSignatures {
                signatures,
                responder,
            },
            QueueKind::ToStorage,
        )
        .await
    }

    pub(crate) async fn put_finality_signature_to_storage(
        self,
        signature: FinalitySignature,
    ) -> bool
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::PutFinalitySignature {
                signature: Box::new(signature),
                responder,
            },
            QueueKind::ToStorage,
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
            QueueKind::FromStorage,
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
            QueueKind::FromStorage,
        )
        .await
    }

    /// Requests the highest complete block.
    pub(crate) async fn get_highest_complete_block_from_storage(self) -> Option<Block>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetHighestCompleteBlock { responder },
            QueueKind::FromStorage,
        )
        .await
    }

    /// Requests the highest complete block header.
    pub(crate) async fn get_highest_complete_block_header_from_storage(self) -> Option<BlockHeader>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetHighestCompleteBlockHeader { responder },
            QueueKind::FromStorage,
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
            QueueKind::FromStorage,
        )
        .await
    }

    /// Synchronize global state under the given root hash.
    pub(crate) async fn sync_global_state(
        self,
        block_hash: BlockHash,
        state_root_hash: Digest,
        peers: HashSet<NodeId>,
    ) -> Result<Digest, GlobalStateSynchronizerError>
    where
        REv: From<SyncGlobalStateRequest>,
    {
        self.make_request(
            |responder| SyncGlobalStateRequest {
                block_hash,
                state_root_hash,
                peers,
                responder,
            },
            QueueKind::SyncGlobalState,
        )
        .await
    }

    /// Get a trie or chunk by its ID.
    pub(crate) async fn get_trie(
        self,
        trie_or_chunk_id: TrieOrChunkId,
    ) -> Result<Option<TrieOrChunk>, ContractRuntimeError>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::GetTrie {
                trie_or_chunk_id,
                responder,
            },
            QueueKind::ContractRuntime,
        )
        .await
    }

    pub(crate) async fn get_reactor_status(self) -> (ReactorState, Timestamp)
    where
        REv: From<ReactorStatusRequest>,
    {
        self.make_request(ReactorStatusRequest, QueueKind::Regular)
            .await
    }

    pub(crate) async fn get_block_synchronizer_status(self) -> BlockSynchronizerStatus
    where
        REv: From<BlockSynchronizerRequest>,
    {
        self.make_request(
            |responder| BlockSynchronizerRequest::Status { responder },
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
            QueueKind::ContractRuntime,
        )
        .await
    }

    /// Puts a trie into the trie store; succeeds only if all the children of the trie are already
    /// present in the store.
    /// Returns the digest under which the trie was stored if successful.
    pub(crate) async fn put_trie_if_all_children_present(
        self,
        trie_bytes: TrieRaw,
    ) -> Result<Digest, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::PutTrie {
                trie_bytes,
                responder,
            },
            QueueKind::ContractRuntime,
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
            QueueKind::ToStorage,
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
            QueueKind::FromStorage,
        )
        .await
    }

    /// Gets the requested deploy from the deploy store by DeployHash only.
    ///
    /// Returns the legacy deploy containing the set of approvals used during execution of the
    /// recorded block, if known.
    pub(crate) async fn get_stored_legacy_deploy(
        self,
        deploy_hash: DeployHash,
    ) -> Option<LegacyDeploy>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetLegacyDeploy {
                deploy_hash,
                responder,
            },
            QueueKind::FromStorage,
        )
        .await
    }

    /// Gets the requested deploy from the deploy store by DeployId.
    ///
    /// Returns the "original" deploy, which are the first received by the node, along with a
    /// potentially different set of approvals used during execution of the recorded block.
    pub(crate) async fn get_stored_deploy(self, deploy_id: DeployId) -> Option<Deploy>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetDeploy {
                deploy_id,
                responder,
            },
            QueueKind::FromStorage,
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
            QueueKind::ToStorage,
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
            QueueKind::FromStorage,
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
            QueueKind::FromStorage,
        )
        .await
    }

    /// Gets the requested finality signature from storage.
    pub(crate) async fn get_finality_signature_from_storage(
        self,
        id: FinalitySignatureId,
    ) -> Option<FinalitySignature>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetFinalitySignature {
                id: Box::new(id),
                responder,
            },
            QueueKind::FromStorage,
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
            QueueKind::FromStorage,
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
            QueueKind::FromStorage,
        )
        .await
    }

    /// Fetches an item from a fetcher.
    pub(crate) async fn fetch<T>(
        self,
        id: T::Id,
        peer: NodeId,
        validation_metadata: T::ValidationMetadata,
    ) -> FetchResult<T>
    where
        REv: From<FetcherRequest<T>>,
        T: FetcherItem + 'static,
    {
        self.make_request(
            |responder| FetcherRequest {
                id,
                peer,
                validation_metadata,
                responder,
            },
            QueueKind::Fetch,
        )
        .await
    }

    pub(crate) async fn fetch_trie(
        self,
        hash: Digest,
        peers: Vec<NodeId>,
    ) -> Result<Box<TrieRaw>, TrieAccumulatorError>
    where
        REv: From<TrieAccumulatorRequest>,
    {
        self.make_request(
            |responder| TrieAccumulatorRequest {
                hash,
                peers,
                responder,
            },
            QueueKind::SyncGlobalState,
        )
        .await
    }

    /// Passes the timestamp of a future block for which deploys are to be proposed.
    pub(crate) async fn request_appendable_block(self, timestamp: Timestamp) -> AppendableBlock
    where
        REv: From<DeployBufferRequest>,
    {
        self.make_request(
            |responder| DeployBufferRequest::GetAppendableBlock {
                timestamp,
                responder,
            },
            QueueKind::Consensus,
        )
        .await
    }

    /// Enqueues a finalized block execution.
    ///
    /// # Arguments
    ///
    /// * `finalized_block` - a finalized block to add to the execution queue.
    /// * `deploys` - a vector of deploys and transactions that match the hashes in the finalized
    ///   block, in that order.
    pub(crate) async fn enqueue_block_for_execution(
        self,
        finalized_block: FinalizedBlock,
        deploys: Vec<Deploy>,
    ) where
        REv: From<ContractRuntimeRequest>,
    {
        self.event_queue
            .schedule(
                ContractRuntimeRequest::EnqueueBlockForExecution {
                    finalized_block,
                    deploys,
                },
                QueueKind::ContractRuntime,
            )
            .await
    }

    /// Checks whether the deploys included in the block exist on the network and the block is
    /// valid.
    pub(crate) async fn validate_block(
        self,
        sender: NodeId,
        block: ProposedBlock<ClContext>,
    ) -> bool
    where
        REv: From<BlockValidationRequest>,
    {
        self.make_request(
            |responder| BlockValidationRequest {
                block,
                sender,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Announces that a block has been proposed.
    pub(crate) async fn announce_proposed_block(self, proposed_block: ProposedBlock<ClContext>)
    where
        REv: From<ConsensusAnnouncement>,
    {
        self.event_queue
            .schedule(
                ConsensusAnnouncement::Proposed(Box::new(proposed_block)),
                QueueKind::Consensus,
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
                QueueKind::Consensus,
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
                QueueKind::Consensus,
            )
            .await
    }

    /// Blocks a specific peer due to a transgression.
    ///
    /// This function will also emit a log message for the block.
    pub(crate) async fn announce_block_peer_with_justification(
        self,
        offender: NodeId,
        justification: BlocklistJustification,
    ) where
        REv: From<PeerBehaviorAnnouncement>,
    {
        warn!(%offender, %justification, "banning peer");
        self.event_queue
            .schedule(
                PeerBehaviorAnnouncement::OffenseCommitted {
                    offender: Box::new(offender),
                    justification: Box::new(justification),
                },
                QueueKind::NetworkInfo,
            )
            .await
    }

    /// Gets the next scheduled upgrade, if any.
    pub(crate) async fn get_next_upgrade(self) -> Option<NextUpgrade>
    where
        REv: From<UpgradeWatcherRequest> + Send,
    {
        self.make_request(UpgradeWatcherRequest, QueueKind::Control)
            .await
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
            QueueKind::ContractRuntime,
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
            QueueKind::ContractRuntime,
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
            QueueKind::ContractRuntime,
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
            QueueKind::ContractRuntime,
        )
        .await
    }

    /// Returns the value of the execution results checksum stored in the ChecksumRegistry for the
    /// given state root hash.
    pub(crate) async fn get_execution_results_checksum(
        self,
        state_root_hash: Digest,
    ) -> Result<Option<Digest>, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::GetExecutionResultsChecksum {
                state_root_hash,
                responder,
            },
            QueueKind::ContractRuntime,
        )
        .await
    }

    /// Get our public key from consensus, and if we're a validator, the next round length.
    pub(crate) async fn consensus_status(self) -> Option<(PublicKey, Option<TimeDiff>)>
    where
        REv: From<ConsensusRequest>,
    {
        self.make_request(ConsensusRequest::Status, QueueKind::Consensus)
            .await
    }

    /// Returns a list of validator status changes, by public key.
    pub(crate) async fn get_consensus_validator_changes(
        self,
    ) -> BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>>
    where
        REv: From<ConsensusRequest>,
    {
        self.make_request(ConsensusRequest::ValidatorChanges, QueueKind::Consensus)
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

    /// Announce that the node be shut down due to a request from a user.
    pub(crate) async fn announce_user_shutdown_request(self)
    where
        REv: From<ControlAnnouncement>,
    {
        self.event_queue
            .schedule(
                ControlAnnouncement::ShutdownDueToUserRequest,
                QueueKind::Control,
            )
            .await;
    }

    /// Get the bytes for the chainspec file and genesis_accounts
    /// and global_state bytes if the files are present.
    pub(crate) async fn get_chainspec_raw_bytes(self) -> Arc<ChainspecRawBytes>
    where
        REv: From<ChainspecRawBytesRequest> + Send,
    {
        self.make_request(
            ChainspecRawBytesRequest::GetChainspecRawBytes,
            QueueKind::NetworkInfo,
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
    ) -> bool
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::StoreFinalizedApprovals {
                deploy_hash,
                finalized_approvals,
                responder,
            },
            QueueKind::ToStorage,
        )
        .await
    }

    /// Requests execution of a single deploy, without commiting its effects.
    /// Inteded to be used for debugging & discovery purposes.
    pub(crate) async fn speculative_execute_deploy(
        self,
        execution_prestate: SpeculativeExecutionState,
        deploy: Deploy,
    ) -> Result<Option<ExecutionResult>, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::SpeculativeDeployExecution {
                execution_prestate,
                deploy: Box::new(deploy),
                responder,
            },
            QueueKind::ContractRuntime,
        )
        .await
    }

    /// Reads block execution results (or chunk) from Storage component.
    pub(crate) async fn get_block_execution_results_or_chunk_from_storage(
        self,
        id: BlockExecutionResultsOrChunkId,
    ) -> Option<BlockExecutionResultsOrChunk>
    where
        REv: From<StorageRequest>, // TODO: Extract to a separate component for caching.
    {
        self.make_request(
            |responder| StorageRequest::GetBlockExecutionResultsOrChunk { id, responder },
            QueueKind::FromStorage,
        )
        .await
    }

    /// Gets peers for a given block from the block accumulator.
    pub(crate) async fn get_block_accumulated_peers(
        self,
        block_hash: BlockHash,
    ) -> Option<Vec<NodeId>>
    where
        REv: From<BlockAccumulatorRequest>,
    {
        self.make_request(
            |responder| BlockAccumulatorRequest::GetPeersForBlock {
                block_hash,
                responder,
            },
            QueueKind::NetworkInfo,
        )
        .await
    }

    /// Set a new stopping point for the node.
    ///
    /// Returns a potentially previously set stop-at spec.
    pub(crate) async fn set_node_stop_at(self, stop_at: Option<StopAtSpec>) -> Option<StopAtSpec>
    where
        REv: From<SetNodeStopRequest>,
    {
        self.make_request(
            |responder| SetNodeStopRequest { stop_at, responder },
            QueueKind::Control,
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
