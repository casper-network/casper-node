//! Effects subsystem.
//!
//! Effects describe things that the creator of the effect intends to happen, producing a value upon
//! completion. They are, in fact, futures.
//!
//! A pinned, boxed future returning an event is called an effect and typed as an `Effect<Ev>`,
//! where `Ev` is the event's type. Generally, `Ev` is an Event enum defined at the top level of
//! each component in the `crate::components` module.
//!
//! ## Using effects
//!
//! To create an effect, an `EffectBuilder` will be passed in from the relevant reactor. For
//! example, given an effect builder `effect_builder`, we can create a `set_timeout` future and turn
//! it into an effect:
//!
//! ```ignore
//! use std::time::Duration;
//! use casper_node::effect::EffectExt;
//!
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
//! ## Arbitrary effects
//!
//! While it is technically possible to turn any future into an effect, it is advisable to only use
//! the effects explicitly listed in this module through traits to create them. Post-processing on
//! effects to turn them into events should also be kept brief.
//!
//! ## Announcements and effects
//!
//! Some effects can be further classified into either announcements or requests, although these
//! properties are not reflected in the type system.
//!
//! **Announcements** are events emitted by components that are essentially "fire-and-forget"; the
//! component will never expect an answer for these and does not rely on them being handled. It is
//! also conceivable that they are being cloned and dispatched to multiple components by the
//! reactor.
//!
//! A good example is the arrival of a new deploy passed in by a client. Depending on the setup it
//! may be stored, buffered or, in certain testing setups, just discarded. None of this is a concern
//! of the component that talks to the client and deserializes the incoming deploy though, which
//! considers the deploy no longer its concern after it has returned an announcement effect.
//!
//! **Requests** are complex effects that are used when a component needs something from
//! outside of itself (typically to be provided by another component); a request requires an
//! eventual response.
//!
//! A request **must** have a `Responder` field, which a handler of a request **must** call at
//! some point. Failing to do so will result in a resource leak.

pub mod announcements;
pub mod requests;

use std::{
    any::type_name,
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use datasize::DataSize;
use futures::{channel::oneshot, future::BoxFuture, FutureExt};
use semver::Version;
use serde::{de::DeserializeOwned, Serialize};
use smallvec::{smallvec, SmallVec};
use tokio::join;
use tracing::{error, warn};

use casper_execution_engine::{
    core::engine_state::{
        self,
        era_validators::GetEraValidatorsError,
        execute_request::ExecuteRequest,
        execution_result::ExecutionResults,
        genesis::GenesisResult,
        step::{StepRequest, StepResult},
        BalanceRequest, BalanceResult, QueryRequest, QueryResult, MAX_PAYMENT,
    },
    shared::{additive_map::AdditiveMap, stored_value::StoredValue, transform::Transform},
    storage::{global_state::CommitResult, protocol_data::ProtocolData},
};
use casper_types::{
    auction::{EraValidators, ValidatorWeights},
    ExecutionResult, Key, ProtocolVersion, Transfer,
};

use crate::{
    components::{
        chainspec_loader::ChainspecInfo,
        consensus::{BlockContext, EraId},
        contract_runtime::{EraValidatorsRequest, ValidatorWeightsByEraIdRequest},
        fetcher::FetchResult,
        small_network::GossipedAddress,
    },
    crypto::{asymmetric_key::PublicKey, hash::Digest},
    effect::requests::LinearChainRequest,
    reactor::{EventQueueHandle, QueueKind},
    types::{
        Block, BlockByHeight, BlockHash, BlockHeader, BlockLike, Deploy, DeployHash, DeployHeader,
        DeployMetadata, FinalitySignature, FinalizedBlock, Item, ProtoBlock, Timestamp,
    },
    utils::Source,
    Chainspec,
};
use announcements::{
    BlockExecutorAnnouncement, ConsensusAnnouncement, DeployAcceptorAnnouncement,
    GossiperAnnouncement, LinearChainAnnouncement, NetworkAnnouncement, RpcServerAnnouncement,
};
use requests::{
    BlockExecutorRequest, BlockProposerRequest, BlockValidationRequest, ChainspecLoaderRequest,
    ConsensusRequest, ContractRuntimeRequest, FetcherRequest, MetricsRequest, NetworkInfoRequest,
    NetworkRequest, ProtoBlockRequest, StateStoreRequest, StorageRequest,
};

/// A pinned, boxed future that produces one or more events.
pub type Effect<Ev> = BoxFuture<'static, Multiple<Ev>>;

/// Multiple effects in a container.
pub type Effects<Ev> = Multiple<Effect<Ev>>;

/// A small collection of rarely more than two items.
///
/// Stored in a `SmallVec` to avoid allocations in case there are less than three items grouped. The
/// size of two items is chosen because one item is the most common use case, and large items are
/// typically boxed. In the latter case two pointers and one enum variant discriminator is almost
/// the same size as an empty vec, which is two pointers.
pub type Multiple<T> = SmallVec<[T; 2]>;

/// A responder satisfying a request.
#[must_use]
#[derive(DataSize)]
pub struct Responder<T>(Option<oneshot::Sender<T>>);

impl<T: 'static + Send> Responder<T> {
    /// Creates a new `Responder`.
    #[inline]
    fn new(sender: oneshot::Sender<T>) -> Self {
        Responder(Some(sender))
    }

    /// Helper method for tests.
    ///
    /// Allows creating a responder manually. This function should not be used, unless you are
    /// writing alternative infrastructure, e.g. for tests.
    #[cfg(test)]
    #[inline]
    pub(crate) fn create(sender: oneshot::Sender<T>) -> Self {
        Responder::new(sender)
    }
}

impl<T> Responder<T> {
    /// Send `data` to the origin of the request.
    pub async fn respond(mut self, data: T) {
        if let Some(sender) = self.0.take() {
            if sender.send(data).is_err() {
                error!("could not send response to request down oneshot channel");
            }
        } else {
            error!("tried to send a value down a responder channel, but it was already used");
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
        if self.0.is_some() {
            // This is usually a very serious error, as another component will now be stuck.
            error!(
                "{} dropped without being responded to --- \
                 this is always a bug and will likely cause another component to be stuck!",
                self
            );
        }
    }
}

impl<T> Serialize for Responder<T> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{:?}", self))
    }
}

/// Effect extension for futures, used to convert futures into actual effects.
pub trait EffectExt: Future + Send {
    /// Finalizes a future into an effect that returns an event.
    ///
    /// The function `f` is used to translate the returned value from an effect into an event.
    fn event<U, F>(self, f: F) -> Effects<U>
    where
        F: FnOnce(Self::Output) -> U + 'static + Send,
        U: 'static,
        Self: Sized;

    /// Finalizes a future into an effect that runs but drops the result.
    fn ignore<Ev>(self) -> Effects<Ev>;
}

/// Effect extension for futures, used to convert futures returning a `Result` into two different
/// effects.
pub trait EffectResultExt {
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
pub trait EffectOptionExt {
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
/// Provides methods allowing the creation of effects which need to be scheduled
/// on the reactor's event queue, without giving direct access to this queue.
#[derive(Debug)]
pub struct EffectBuilder<REv: 'static>(EventQueueHandle<REv>);

// Implement `Clone` and `Copy` manually, as `derive` will make it depend on `REv` otherwise.
impl<REv> Clone for EffectBuilder<REv> {
    fn clone(&self) -> Self {
        EffectBuilder(self.0)
    }
}

impl<REv> Copy for EffectBuilder<REv> {}

impl<REv> EffectBuilder<REv> {
    /// Creates a new effect builder.
    pub fn new(event_queue_handle: EventQueueHandle<REv>) -> Self {
        EffectBuilder(event_queue_handle)
    }

    /// Performs a request.
    ///
    /// Given a request `Q`, that when completed will yield a result of `T`, produces a future
    /// that will
    ///
    /// 1. create an event to send the request to the respective component (thus `Q: Into<REv>`),
    /// 2. waits for a response and returns it.
    ///
    /// This function is usually only used internally by effects implement on the effects builder,
    /// but IO components may also make use of it.
    pub(crate) async fn make_request<T, Q, F>(self, f: F, queue_kind: QueueKind) -> T
    where
        T: Send + 'static,
        Q: Into<REv>,
        F: FnOnce(Responder<T>) -> Q,
    {
        // Prepare a channel.
        let (sender, receiver) = oneshot::channel();

        // Create response function.
        let responder = Responder::new(sender);

        // Now inject the request event into the event loop.
        let request_event = f(responder).into();
        self.0.schedule(request_event, queue_kind).await;

        receiver.await.unwrap_or_else(|err| {
            // The channel should never be closed, ever.
            error!(%err, ?queue_kind, "request for {} channel closed, this is a serious bug --- \
                   a component will likely be stuck from now on ", type_name::<T>());

            // We cannot produce any value to satisfy the request, so all that's left is panicking.
            panic!("request not answerable");
        })
    }

    /// Run and end effect immediately.
    ///
    /// Can be used to trigger events from effects when combined with `.event`. Do not use this do
    /// "do nothing", as it will still cause a task to be spawned.
    #[inline(always)]
    #[allow(clippy::manual_async_fn)]
    pub fn immediately(self) -> impl Future<Output = ()> + Send {
        // Note: This function is implemented manually without `async` sugar because the `Send`
        // inference seems to not work in all cases otherwise.
        async {}
    }

    /// Reports a fatal error.  Normally called via the `crate::fatal!()` macro.
    ///
    /// Usually causes the node to cease operations quickly and exit/crash.
    //
    // Note: This function is implemented manually without `async` sugar because the `Send`
    // inferrence seems to not work in all cases otherwise.
    pub fn fatal(self, file: &str, line: u32, msg: String) -> impl Future<Output = ()> + Send {
        panic!("fatal error [{}:{}]: {}", file, line, msg);
        #[allow(unreachable_code)]
        async {} // The compiler will complain about an incorrect return value otherwise.
    }

    /// Sets a timeout.
    pub(crate) async fn set_timeout(self, timeout: Duration) -> Duration {
        let then = Instant::now();
        tokio::time::delay_for(timeout).await;
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

    /// Retrieves block at `height` from the Linear Chain component.
    pub(crate) async fn get_block_at_height_local<I>(self, height: u64) -> Option<Block>
    where
        REv: From<LinearChainRequest<I>>,
    {
        self.make_request(
            |responder| LinearChainRequest::BlockAtHeightLocal(height, responder),
            QueueKind::Regular,
        )
        .await
    }

    /// Sends a network message.
    ///
    /// The message is queued in "fire-and-forget" fashion, there is no guarantee that the peer
    /// will receive it.
    pub(crate) async fn send_message<I, P>(self, dest: I, payload: P)
    where
        REv: From<NetworkRequest<I, P>>,
    {
        self.make_request(
            |responder| NetworkRequest::SendMessage {
                dest,
                payload,
                responder,
            },
            QueueKind::Network,
        )
        .await
    }

    /// Broadcasts a network message.
    ///
    /// Broadcasts a network message to all peers connected at the time the message is sent.
    pub async fn broadcast_message<I, P>(self, payload: P)
    where
        REv: From<NetworkRequest<I, P>>,
    {
        self.make_request(
            |responder| NetworkRequest::Broadcast { payload, responder },
            QueueKind::Network,
        )
        .await
    }

    /// Gossips a network message.
    ///
    /// A low-level "gossip" function, selects `count` randomly chosen nodes on the network,
    /// excluding the indicated ones, and sends each a copy of the message.
    ///
    /// Returns the IDs of the chosen nodes.
    pub async fn gossip_message<I, P>(
        self,
        payload: P,
        count: usize,
        exclude: HashSet<I>,
    ) -> HashSet<I>
    where
        REv: From<NetworkRequest<I, P>>,
        I: Send + 'static,
        P: Send,
    {
        self.make_request(
            |responder| NetworkRequest::Gossip {
                payload,
                count,
                exclude,
                responder,
            },
            QueueKind::Network,
        )
        .await
    }

    /// Gets connected network peers.
    pub async fn network_peers<I>(self) -> BTreeMap<I, String>
    where
        REv: From<NetworkInfoRequest<I>>,
        I: Send + 'static,
    {
        self.make_request(
            |responder| NetworkInfoRequest::GetPeers { responder },
            QueueKind::Api,
        )
        .await
    }

    /// Announces that a network message has been received.
    pub(crate) async fn announce_message_received<I, P>(self, sender: I, payload: P)
    where
        REv: From<NetworkAnnouncement<I, P>>,
    {
        self.0
            .schedule(
                NetworkAnnouncement::MessageReceived { sender, payload },
                QueueKind::NetworkIncoming,
            )
            .await;
    }

    /// Announces that we should gossip our own public listening address.
    pub(crate) async fn announce_gossip_our_address<I, P>(self, our_address: GossipedAddress)
    where
        REv: From<NetworkAnnouncement<I, P>>,
    {
        self.0
            .schedule(
                NetworkAnnouncement::GossipOurAddress(our_address),
                QueueKind::Regular,
            )
            .await;
    }

    /// Announces that a new peer has connected.
    pub(crate) async fn announce_new_peer<I, P>(self, peer_id: I)
    where
        REv: From<NetworkAnnouncement<I, P>>,
    {
        self.0
            .schedule(
                NetworkAnnouncement::NewPeer(peer_id),
                QueueKind::NetworkIncoming,
            )
            .await;
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
        self.0
            .schedule(
                GossiperAnnouncement::NewCompleteItem(item),
                QueueKind::Regular,
            )
            .await;
    }

    /// Announces that the HTTP API server has received a deploy.
    pub(crate) async fn announce_deploy_received(self, deploy: Box<Deploy>)
    where
        REv: From<RpcServerAnnouncement>,
    {
        self.0
            .schedule(
                RpcServerAnnouncement::DeployReceived { deploy },
                QueueKind::Api,
            )
            .await;
    }

    /// Announces that a deploy not previously stored has now been accepted and stored.
    pub(crate) fn announce_new_deploy_accepted<I>(
        self,
        deploy: Box<Deploy>,
        source: Source<I>,
    ) -> impl Future<Output = ()>
    where
        REv: From<DeployAcceptorAnnouncement<I>>,
    {
        self.0.schedule(
            DeployAcceptorAnnouncement::AcceptedNewDeploy { deploy, source },
            QueueKind::Regular,
        )
    }

    /// Announces that an invalid deploy has been received.
    pub(crate) fn announce_invalid_deploy<I>(
        self,
        deploy: Box<Deploy>,
        source: Source<I>,
    ) -> impl Future<Output = ()>
    where
        REv: From<DeployAcceptorAnnouncement<I>>,
    {
        self.0.schedule(
            DeployAcceptorAnnouncement::InvalidDeploy { deploy, source },
            QueueKind::Regular,
        )
    }

    /// Announce new block has been created.
    pub(crate) async fn announce_linear_chain_block(
        self,
        block: Block,
        execution_results: HashMap<DeployHash, (DeployHeader, ExecutionResult)>,
    ) where
        REv: From<BlockExecutorAnnouncement>,
    {
        self.0
            .schedule(
                BlockExecutorAnnouncement::LinearChainBlock {
                    block,
                    execution_results,
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

    /// Requests block at height.
    pub(crate) async fn get_block_at_height(self, height: u64) -> Option<Block>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetBlockAtHeight { height, responder },
            QueueKind::Regular,
        )
        .await
    }

    /// Requests the highest block.
    pub(crate) async fn get_highest_block(self) -> Option<Block>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetHighestBlock { responder },
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
    pub(crate) async fn get_deploys_from_storage(
        self,
        deploy_hashes: Multiple<DeployHash>,
    ) -> Vec<Option<Deploy>>
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
                block_hash,
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
    ) -> Option<(Deploy, DeployMetadata)>
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

    /// Gets the requested deploy using the `DeployFetcher`.
    pub(crate) async fn fetch_deploy<I>(
        self,
        deploy_hash: DeployHash,
        peer: I,
    ) -> Option<FetchResult<Deploy>>
    where
        REv: From<FetcherRequest<I, Deploy>>,
        I: Send + 'static,
    {
        self.make_request(
            |responder| FetcherRequest::Fetch {
                id: deploy_hash,
                peer,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the requested block using the `BlockFetcher`
    pub(crate) async fn fetch_block<I>(
        self,
        block_hash: BlockHash,
        peer: I,
    ) -> Option<FetchResult<Block>>
    where
        REv: From<FetcherRequest<I, Block>>,
        I: Send + 'static,
    {
        self.make_request(
            |responder| FetcherRequest::Fetch {
                id: block_hash,
                peer,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Requests a linear chain block at `block_height`.
    pub(crate) async fn fetch_block_by_height<I>(
        self,
        block_height: u64,
        peer: I,
    ) -> Option<FetchResult<BlockByHeight>>
    where
        REv: From<FetcherRequest<I, BlockByHeight>>,
        I: Send + 'static,
    {
        self.make_request(
            |responder| FetcherRequest::Fetch {
                id: block_height,
                peer,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Passes the timestamp of a future block for which deploys are to be proposed.
    pub(crate) async fn request_proto_block(
        self,
        block_context: BlockContext,
        past_deploys: HashSet<DeployHash>,
        next_finalized: u64,
        random_bit: bool,
    ) -> (ProtoBlock, BlockContext)
    where
        REv: From<BlockProposerRequest>,
    {
        let proto_block = self
            .make_request(
                |responder| {
                    BlockProposerRequest::RequestProtoBlock(ProtoBlockRequest {
                        current_instant: block_context.timestamp(),
                        past_deploys,
                        next_finalized,
                        responder,
                        random_bit,
                    })
                },
                QueueKind::Regular,
            )
            .await;
        (proto_block, block_context)
    }

    /// Passes a finalized proto-block to the block executor component to execute it.
    pub(crate) async fn execute_block(self, finalized_block: FinalizedBlock)
    where
        REv: From<BlockExecutorRequest>,
    {
        self.0
            .schedule(
                BlockExecutorRequest::ExecuteBlock(finalized_block),
                QueueKind::Regular,
            )
            .await
    }

    /// Checks whether the deploys included in the block exist on the network. This includes
    /// the block's timestamp, in order that it be checked against the timestamp of the deploys
    /// within the block.
    pub(crate) async fn validate_block<I, T>(
        self,
        sender: I,
        block: T,
        block_timestamp: Timestamp,
    ) -> (bool, T)
    where
        REv: From<BlockValidationRequest<T, I>>,
        T: BlockLike + Send + 'static,
    {
        self.make_request(
            |responder| BlockValidationRequest {
                block,
                sender,
                responder,
                block_timestamp,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Announces that a proto block has been finalized.
    pub(crate) async fn announce_finalized_block<I>(self, finalized_block: FinalizedBlock)
    where
        REv: From<ConsensusAnnouncement<I>>,
    {
        self.0
            .schedule(
                ConsensusAnnouncement::Finalized(Box::new(finalized_block)),
                QueueKind::Regular,
            )
            .await
    }

    pub(crate) async fn announce_block_handled<I>(self, block_header: BlockHeader)
    where
        REv: From<ConsensusAnnouncement<I>>,
    {
        self.0
            .schedule(
                ConsensusAnnouncement::Handled(Box::new(block_header)),
                QueueKind::Regular,
            )
            .await
    }

    /// An equivocation has been detected.
    pub(crate) async fn announce_fault_event<I>(
        self,
        era_id: EraId,
        public_key: PublicKey,
        timestamp: Timestamp,
    ) where
        REv: From<ConsensusAnnouncement<I>>,
    {
        self.0
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
    pub(crate) async fn announce_disconnect_from_peer<I>(self, peer: I)
    where
        REv: From<ConsensusAnnouncement<I>>,
    {
        self.0
            .schedule(
                ConsensusAnnouncement::DisconnectFromPeer(peer),
                QueueKind::Regular,
            )
            .await
    }

    /// The linear chain has stored a newly-created block.
    pub(crate) async fn announce_block_added(self, block_hash: BlockHash, block_header: BlockHeader)
    where
        REv: From<LinearChainAnnouncement>,
    {
        self.0
            .schedule(
                LinearChainAnnouncement::BlockAdded {
                    block_hash,
                    block_header: Box::new(block_header),
                },
                QueueKind::Regular,
            )
            .await
    }

    /// The linear chain has stored a new finality signature.
    pub(crate) async fn announce_finality_signature(self, fs: Box<FinalitySignature>)
    where
        REv: From<LinearChainAnnouncement>,
    {
        self.0
            .schedule(
                LinearChainAnnouncement::NewFinalitySignature(fs),
                QueueKind::Regular,
            )
            .await
    }

    /// Runs the genesis process on the contract runtime.
    pub(crate) async fn commit_genesis(
        self,
        chainspec: Chainspec,
    ) -> Result<GenesisResult, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::CommitGenesis {
                chainspec: Box::new(chainspec),
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Puts the given chainspec into the chainspec store.
    pub(crate) async fn put_chainspec(self, chainspec: Chainspec)
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::PutChainspec {
                chainspec: Arc::new(chainspec),
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the requested chainspec from the chainspec store.
    pub(crate) async fn get_chainspec(self, version: Version) -> Option<Arc<Chainspec>>
    where
        REv: From<StorageRequest>,
    {
        self.make_request(
            |responder| StorageRequest::GetChainspec { version, responder },
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

    /// Loads potentially previously stored state from storage.
    ///
    /// Key must be a unique key across the the application, as all keys share a common namespace.
    ///
    /// If an error occurs during state loading or no data is found, returns `None`.
    pub(crate) async fn load_state<T>(self, key: Cow<'static, [u8]>) -> Option<T>
    where
        REv: From<StateStoreRequest>,
        T: DeserializeOwned,
    {
        // There is an ugly truth hidden in here: Due to object safety issues, we cannot ship the
        // actual values around, but only the serialized bytes. For this reason this function
        // retrieves raw bytes from storage and perform deserialization here.
        //
        // Errors are prominently logged but not treated further in any way.
        self.make_request(
            move |responder| StateStoreRequest::Load { key, responder },
            QueueKind::Regular,
        )
        .await
        .map(|data| bincode::deserialize(&data))
        .transpose()
        .unwrap_or_else(|err| {
            let type_name = type_name::<T>();
            warn!(%type_name, %err, "could not deserialize state from storage");
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
                warn!(%type_name, %err, "Error serializing state");
                false
            }
        }
    }

    /// Requests an execution of deploys using Contract Runtime.
    pub(crate) async fn request_execute(
        self,
        execute_request: ExecuteRequest,
    ) -> Result<ExecutionResults, engine_state::RootNotFound>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::Execute {
                execute_request,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Requests a commit of effects on the Contract Runtime component.
    pub(crate) async fn request_commit(
        self,
        state_root_hash: Digest,
        effects: AdditiveMap<Key, Transform>,
    ) -> Result<CommitResult, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::Commit {
                state_root_hash,
                effects,
                responder,
            },
            QueueKind::Regular,
        )
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
            QueueKind::Regular,
        )
        .await
    }

    pub(crate) async fn is_verified_account(self, account_key: Key) -> bool
    where
        REv: From<ContractRuntimeRequest>,
        REv: From<StorageRequest>,
    {
        if let Some(block) = self.get_highest_block().await {
            let state_hash = (*block.state_root_hash()).into();
            let query_request = QueryRequest::new(state_hash, account_key, vec![]);
            if let Ok(QueryResult::Success { value, .. }) =
                self.query_global_state(query_request).await
            {
                if let StoredValue::Account(account) = *value {
                    let purse_uref = account.main_purse();
                    let balance_request = BalanceRequest::new(state_hash, purse_uref);
                    if let Ok(balance_result) = self.get_balance(balance_request).await {
                        if let Some(motes) = balance_result.motes() {
                            return motes >= &*MAX_PAYMENT;
                        }
                    }
                }
            }
        }
        false
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

    /// Returns `ProtocolData` by `ProtocolVersion`.
    ///
    /// This operation is read only.
    pub(crate) async fn get_protocol_data(
        self,
        protocol_version: ProtocolVersion,
    ) -> Result<Option<Box<ProtocolData>>, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::GetProtocolData {
                protocol_version,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Returns a map of validators weights for all eras as known from `root_hash`.
    ///
    /// This operation is read only.
    pub(crate) async fn get_era_validators(
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

    /// Returns a map of validators for given `era` to their weights as known from `root_hash`.
    ///
    /// This operation is read only.
    pub(crate) async fn get_validator_weights_by_era_id(
        self,
        request: ValidatorWeightsByEraIdRequest,
    ) -> Result<Option<ValidatorWeights>, GetEraValidatorsError>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::GetValidatorWeightsByEraId { request, responder },
            QueueKind::Regular,
        )
        .await
    }

    /// Runs the end of era step using the system smart contract.
    pub(crate) async fn run_step(
        self,
        step_request: StepRequest,
    ) -> Result<StepResult, engine_state::Error>
    where
        REv: From<ContractRuntimeRequest>,
    {
        self.make_request(
            |responder| ContractRuntimeRequest::Step {
                step_request,
                responder,
            },
            QueueKind::Regular,
        )
        .await
    }

    /// Gets the set of validators, the booking block and the key block for a new era
    pub(crate) async fn create_new_era(
        self,
        request: ValidatorWeightsByEraIdRequest,
        booking_block_height: u64,
        key_block_height: u64,
    ) -> (
        Result<Option<ValidatorWeights>, GetEraValidatorsError>,
        Option<Block>,
        Option<Block>,
    )
    where
        REv: From<ContractRuntimeRequest> + From<StorageRequest>,
    {
        let future_validators = self.get_validator_weights_by_era_id(request);
        let future_booking_block = self.get_block_at_height(booking_block_height);
        let future_key_block = self.get_block_at_height(key_block_height);
        join!(future_validators, future_booking_block, future_key_block)
    }

    /// Request consensus to sign a block from the linear chain and possibly start a new era.
    pub(crate) async fn handle_linear_chain_block(
        self,
        block_header: BlockHeader,
    ) -> Option<FinalitySignature>
    where
        REv: From<ConsensusRequest>,
    {
        self.make_request(
            |responder| ConsensusRequest::HandleLinearBlock(Box::new(block_header), responder),
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
        $effect_builder.fatal(file!(), line!(), format_args!($($arg)*).to_string()).ignore()
    };
}
