// Unrestricted event size is okay in tests.
#![allow(clippy::large_enum_variant)]
#![cfg(test)]
use std::{
    collections::{BTreeSet, HashMap},
    iter,
    sync::Arc,
};

use derive_more::{Display, From};
use prometheus::Registry;
use rand::Rng;
use reactor::ReactorEvent;
use serde::Serialize;
use tempfile::TempDir;
use thiserror::Error;
use tokio::time;
use tracing::debug;

use casper_types::{
    testing::TestRng, BlockV2, Chainspec, ChainspecRawBytes, EraId, FinalitySignatureV2,
    ProtocolVersion, TimeDiff, Transaction,
};

use super::*;
use crate::{
    components::{
        in_memory_network::{self, InMemoryNetwork, NetworkController},
        network::{GossipedAddress, Identity as NetworkIdentity},
        storage::{self, Storage},
        transaction_acceptor,
    },
    effect::{
        announcements::{
            ControlAnnouncement, FatalAnnouncement, GossiperAnnouncement,
            TransactionAcceptorAnnouncement,
        },
        incoming::{
            ConsensusDemand, ConsensusMessageIncoming, FinalitySignatureIncoming,
            NetRequestIncoming, NetResponseIncoming, TrieDemand, TrieRequestIncoming,
            TrieResponseIncoming,
        },
        requests::AcceptTransactionRequest,
    },
    protocol::Message as NodeMessage,
    reactor::{self, EventQueueHandle, QueueKind, Runner, TryCrankOutcome},
    testing::{
        self,
        network::{NetworkedReactor, TestingNetwork},
        ConditionCheckReactor, FakeTransactionAcceptor,
    },
    types::NodeId,
    utils::WithDir,
    NodeRng,
};

const RECENT_ERA_COUNT: u64 = 5;
const MAX_TTL: TimeDiff = TimeDiff::from_seconds(86400);
const EXPECTED_GOSSIP_TARGET: GossipTarget = GossipTarget::All;

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize, Display)]
#[must_use]
enum Event {
    #[from]
    Network(in_memory_network::Event<NodeMessage>),
    #[from]
    Storage(storage::Event),
    #[from]
    TransactionAcceptor(#[serde(skip_serializing)] transaction_acceptor::Event),
    #[from]
    TransactionGossiper(super::Event<Transaction>),
    #[from]
    NetworkRequest(NetworkRequest<NodeMessage>),
    #[from]
    StorageRequest(StorageRequest),
    #[from]
    AcceptTransactionRequest(AcceptTransactionRequest),
    #[from]
    TransactionAcceptorAnnouncement(#[serde(skip_serializing)] TransactionAcceptorAnnouncement),
    #[from]
    TransactionGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<Transaction>),
    #[from]
    TransactionGossiperIncoming(GossiperIncoming<Transaction>),
}

impl ReactorEvent for Event {
    fn is_control(&self) -> bool {
        false
    }

    fn try_into_control(self) -> Option<ControlAnnouncement> {
        None
    }
}

impl From<NetworkRequest<Message<Transaction>>> for Event {
    fn from(request: NetworkRequest<Message<Transaction>>) -> Self {
        Event::NetworkRequest(request.map_payload(NodeMessage::from))
    }
}

trait Unhandled {}

impl<T: Unhandled> From<T> for Event {
    fn from(_: T) -> Self {
        unimplemented!("not handled in gossiper tests")
    }
}

impl Unhandled for ConsensusDemand {}
impl Unhandled for ControlAnnouncement {}
impl Unhandled for FatalAnnouncement {}
impl Unhandled for ConsensusMessageIncoming {}
impl Unhandled for GossiperIncoming<BlockV2> {}
impl Unhandled for GossiperIncoming<FinalitySignatureV2> {}
impl Unhandled for GossiperIncoming<GossipedAddress> {}
impl Unhandled for NetRequestIncoming {}
impl Unhandled for NetResponseIncoming {}
impl Unhandled for TrieRequestIncoming {}
impl Unhandled for TrieDemand {}
impl Unhandled for TrieResponseIncoming {}
impl Unhandled for FinalitySignatureIncoming {}

/// Error type returned by the test reactor.
#[derive(Debug, Error)]
enum Error {
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),
}

struct Reactor {
    network: InMemoryNetwork<NodeMessage>,
    storage: Storage,
    fake_transaction_acceptor: FakeTransactionAcceptor,
    transaction_gossiper: Gossiper<{ Transaction::ID_IS_COMPLETE_ITEM }, Transaction>,
    _storage_tempdir: TempDir,
}

impl Drop for Reactor {
    fn drop(&mut self) {
        NetworkController::<NodeMessage>::remove_node(&self.network.node_id())
    }
}

impl reactor::Reactor for Reactor {
    type Event = Event;
    type Config = Config;
    type Error = Error;

    fn new(
        config: Self::Config,
        _chainspec: Arc<Chainspec>,
        _chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        _network_identity: NetworkIdentity,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let (storage_config, storage_tempdir) = storage::Config::new_for_tests(1);
        let storage_withdir = WithDir::new(storage_tempdir.path(), storage_config);
        let storage = Storage::new(
            &storage_withdir,
            None,
            ProtocolVersion::from_parts(1, 0, 0),
            EraId::default(),
            "test",
            MAX_TTL.into(),
            RECENT_ERA_COUNT,
            Some(registry),
            false,
        )
        .unwrap();

        let fake_transaction_acceptor = FakeTransactionAcceptor::new();
        let transaction_gossiper = Gossiper::<{ Transaction::ID_IS_COMPLETE_ITEM }, _>::new(
            "transaction_gossiper",
            config,
            registry,
        )?;

        let network = NetworkController::create_node(event_queue, rng);
        let reactor = Reactor {
            network,
            storage,
            fake_transaction_acceptor,
            transaction_gossiper,
            _storage_tempdir: storage_tempdir,
        };

        Ok((reactor, Effects::new()))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Event,
    ) -> Effects<Self::Event> {
        trace!(?event);
        match event {
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::TransactionAcceptor(event) => reactor::wrap_effects(
                Event::TransactionAcceptor,
                self.fake_transaction_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            Event::TransactionGossiper(super::Event::ItemReceived {
                item_id,
                source,
                target,
            }) => {
                // Ensure the correct target type for transactions is provided.
                assert_eq!(target, EXPECTED_GOSSIP_TARGET);
                let event = super::Event::ItemReceived {
                    item_id,
                    source,
                    target,
                };
                reactor::wrap_effects(
                    Event::TransactionGossiper,
                    self.transaction_gossiper
                        .handle_event(effect_builder, rng, event),
                )
            }
            Event::TransactionGossiper(event) => reactor::wrap_effects(
                Event::TransactionGossiper,
                self.transaction_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            Event::NetworkRequest(NetworkRequest::Gossip {
                payload,
                gossip_target,
                count,
                exclude,
                auto_closing_responder,
            }) => {
                // Ensure the correct target type for transactions is carried through to the
                // `Network`.
                assert_eq!(gossip_target, EXPECTED_GOSSIP_TARGET);
                let request = NetworkRequest::Gossip {
                    payload,
                    gossip_target,
                    count,
                    exclude,
                    auto_closing_responder,
                };
                reactor::wrap_effects(
                    Event::Network,
                    self.network
                        .handle_event(effect_builder, rng, request.into()),
                )
            }
            Event::NetworkRequest(request) => reactor::wrap_effects(
                Event::Network,
                self.network
                    .handle_event(effect_builder, rng, request.into()),
            ),
            Event::StorageRequest(request) => reactor::wrap_effects(
                Event::Storage,
                self.storage
                    .handle_event(effect_builder, rng, request.into()),
            ),
            Event::AcceptTransactionRequest(AcceptTransactionRequest {
                transaction,
                speculative_exec_at_block,
                responder,
            }) => {
                assert!(speculative_exec_at_block.is_none());
                let event = transaction_acceptor::Event::Accept {
                    transaction,
                    source: Source::Client,
                    maybe_responder: Some(responder),
                };
                self.dispatch_event(effect_builder, rng, Event::TransactionAcceptor(event))
            }
            Event::TransactionAcceptorAnnouncement(
                TransactionAcceptorAnnouncement::AcceptedNewTransaction {
                    transaction,
                    source,
                },
            ) => {
                let event = super::Event::ItemReceived {
                    item_id: transaction.gossip_id(),
                    source,
                    target: transaction.gossip_target(),
                };
                self.dispatch_event(effect_builder, rng, Event::TransactionGossiper(event))
            }
            Event::TransactionAcceptorAnnouncement(
                TransactionAcceptorAnnouncement::InvalidTransaction {
                    transaction: _,
                    source: _,
                },
            ) => Effects::new(),
            Event::TransactionGossiperAnnouncement(GossiperAnnouncement::NewItemBody {
                item,
                sender,
            }) => reactor::wrap_effects(
                Event::TransactionAcceptor,
                self.fake_transaction_acceptor.handle_event(
                    effect_builder,
                    rng,
                    transaction_acceptor::Event::Accept {
                        transaction: *item,
                        source: Source::Peer(sender),
                        maybe_responder: None,
                    },
                ),
            ),
            Event::TransactionGossiperAnnouncement(_ann) => Effects::new(),
            Event::Network(event) => reactor::wrap_effects(
                Event::Network,
                self.network.handle_event(effect_builder, rng, event),
            ),
            Event::TransactionGossiperIncoming(incoming) => reactor::wrap_effects(
                Event::TransactionGossiper,
                self.transaction_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
        }
    }
}

impl NetworkedReactor for Reactor {
    fn node_id(&self) -> NodeId {
        self.network.node_id()
    }
}

fn announce_transaction_received(
    transaction: &Transaction,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    let txn = transaction.clone();
    |effect_builder: EffectBuilder<Event>| effect_builder.try_accept_transaction(txn, None).ignore()
}

async fn run_gossip(rng: &mut TestRng, network_size: usize, txn_count: usize) {
    const TIMEOUT: Duration = Duration::from_secs(20);
    const QUIET_FOR: Duration = Duration::from_millis(50);

    NetworkController::<NodeMessage>::create_active();
    let mut network = TestingNetwork::<Reactor>::new();

    // Add `network_size` nodes.
    let node_ids = network.add_nodes(rng, network_size).await;

    // Create `txn_count` random transactions.
    let (all_txn_hashes, mut txns): (BTreeSet<_>, Vec<_>) = iter::repeat_with(|| {
        let txn = Transaction::random(rng);
        (txn.hash(), txn)
    })
    .take(txn_count)
    .unzip();

    // Give each transaction to a randomly-chosen node to be gossiped.
    for txn in txns.drain(..) {
        let index: usize = rng.gen_range(0..network_size);
        network
            .process_injected_effect_on(&node_ids[index], announce_transaction_received(&txn))
            .await;
    }

    // Check every node has every transaction stored locally.
    let all_txns_held = |nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
        nodes.values().all(|runner| {
            let hashes = runner
                .reactor()
                .inner()
                .storage
                .get_all_transaction_hashes();
            all_txn_hashes == hashes
        })
    };
    network.settle_on(rng, all_txns_held, TIMEOUT).await;

    // Ensure all responders are called before dropping the network.
    network.settle(rng, QUIET_FOR, TIMEOUT).await;

    NetworkController::<NodeMessage>::remove_active();
}

#[tokio::test]
async fn should_gossip() {
    const NETWORK_SIZES: [usize; 3] = [2, 5, 10];
    const TXN_COUNTS: [usize; 3] = [1, 10, 30];

    let rng = &mut TestRng::new();

    for network_size in &NETWORK_SIZES {
        for txn_count in &TXN_COUNTS {
            run_gossip(rng, *network_size, *txn_count).await
        }
    }
}

#[tokio::test]
async fn should_get_from_alternate_source() {
    const NETWORK_SIZE: usize = 3;
    const POLL_DURATION: Duration = Duration::from_millis(10);
    const TIMEOUT: Duration = Duration::from_secs(2);

    NetworkController::<NodeMessage>::create_active();
    let mut network = TestingNetwork::<Reactor>::new();
    let rng = &mut TestRng::new();

    // Add `NETWORK_SIZE` nodes.
    let node_ids = network.add_nodes(rng, NETWORK_SIZE).await;

    // Create random transaction.
    let txn = Transaction::random(rng);
    let txn_hash = txn.hash();

    // Give the transaction to nodes 0 and 1 to be gossiped.
    for node_id in node_ids.iter().take(2) {
        network
            .process_injected_effect_on(node_id, announce_transaction_received(&txn))
            .await;
    }

    // Run node 0 until it has sent the gossip request then remove it from the network.
    let made_gossip_request = |event: &Event| -> bool {
        matches!(event, Event::NetworkRequest(NetworkRequest::Gossip { .. }))
    };
    network
        .crank_until(&node_ids[0], rng, made_gossip_request, TIMEOUT)
        .await;
    assert!(network.remove_node(&node_ids[0]).is_some());
    debug!("removed node {}", &node_ids[0]);

    // Run node 2 until it receives and responds to the gossip request from node 0.
    let node_id_0 = node_ids[0];
    let sent_gossip_response = move |event: &Event| -> bool {
        match event {
            Event::NetworkRequest(NetworkRequest::SendMessage { dest, payload, .. }) => {
                if let NodeMessage::TransactionGossiper(Message::GossipResponse { .. }) = **payload
                {
                    **dest == node_id_0
                } else {
                    false
                }
            }
            _ => false,
        }
    };
    network
        .crank_until(&node_ids[2], rng, sent_gossip_response, TIMEOUT)
        .await;

    // Run nodes 1 and 2 until settled.  Node 2 will be waiting for the transaction from node 0.
    network.settle(rng, POLL_DURATION, TIMEOUT).await;

    // Advance time to trigger node 2's timeout causing it to request the transaction from node 1.
    let duration_to_advance = Config::default().get_remainder_timeout();
    testing::advance_time(duration_to_advance.into()).await;

    // Check node 0 has the transaction stored locally.
    let txn_held = |nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
        let runner = nodes.get(&node_ids[2]).unwrap();
        runner
            .reactor()
            .inner()
            .storage
            .get_transaction_by_hash(txn_hash)
            .map(|retrieved_txn| retrieved_txn == txn)
            .unwrap_or_default()
    };
    network.settle_on(rng, txn_held, TIMEOUT).await;

    NetworkController::<NodeMessage>::remove_active();
}

#[tokio::test]
async fn should_timeout_gossip_response() {
    const PAUSE_DURATION: Duration = Duration::from_millis(50);
    const TIMEOUT: Duration = Duration::from_secs(2);

    NetworkController::<NodeMessage>::create_active();
    let mut network = TestingNetwork::<Reactor>::new();
    let rng = &mut TestRng::new();

    // The target number of peers to infect with a given piece of data.
    let infection_target = Config::default().infection_target();

    // Add `infection_target + 1` nodes.
    let mut node_ids = network.add_nodes(rng, infection_target as usize + 1).await;

    // Create random transaction.
    let txn = Transaction::random(rng);
    let txn_hash = txn.hash();

    // Give the transaction to node 0 to be gossiped.
    network
        .process_injected_effect_on(&node_ids[0], announce_transaction_received(&txn))
        .await;

    // Run node 0 until it has sent the gossip requests.
    let made_gossip_request = |event: &Event| -> bool {
        matches!(
            event,
            Event::TransactionGossiper(super::Event::GossipedTo { .. })
        )
    };
    network
        .crank_until(&node_ids[0], rng, made_gossip_request, TIMEOUT)
        .await;
    // Give node 0 time to set the timeouts before advancing the clock.
    time::sleep(PAUSE_DURATION).await;

    // Replace all nodes except node 0 with new nodes.
    for node_id in node_ids.drain(1..) {
        assert!(network.remove_node(&node_id).is_some());
        debug!("removed node {}", node_id);
    }
    for _ in 0..infection_target {
        let (node_id, _runner) = network.add_node(rng).await.unwrap();
        node_ids.push(node_id);
    }

    // Advance time to trigger node 0's timeout causing it to gossip to the new nodes.
    let duration_to_advance = Config::default().gossip_request_timeout();
    testing::advance_time(duration_to_advance.into()).await;

    // Check every node has every transaction stored locally.
    let txn_held = |nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
        nodes.values().all(|runner| {
            runner
                .reactor()
                .inner()
                .storage
                .get_transaction_by_hash(txn_hash)
                .map(|retrieved_txn| retrieved_txn == txn)
                .unwrap_or_default()
        })
    };
    network.settle_on(rng, txn_held, TIMEOUT).await;

    NetworkController::<NodeMessage>::remove_active();
}

#[tokio::test]
async fn should_timeout_new_item_from_peer() {
    const NETWORK_SIZE: usize = 2;
    const VALIDATE_AND_STORE_TIMEOUT: Duration = Duration::from_secs(1);
    const TIMEOUT: Duration = Duration::from_secs(5);

    NetworkController::<NodeMessage>::create_active();
    let mut network = TestingNetwork::<Reactor>::new();
    let rng = &mut TestRng::new();

    let node_ids = network.add_nodes(rng, NETWORK_SIZE).await;
    let node_0 = node_ids[0];
    let node_1 = node_ids[1];
    // Set the timeout on node 0 low for testing.
    let reactor_0 = network
        .nodes_mut()
        .get_mut(&node_0)
        .unwrap()
        .reactor_mut()
        .inner_mut();
    reactor_0.transaction_gossiper.validate_and_store_timeout = VALIDATE_AND_STORE_TIMEOUT;
    // Switch off the fake transaction acceptor on node 0 so that once the new transaction is
    // received, no component triggers the `ItemReceived` event.
    reactor_0.fake_transaction_acceptor.set_active(false);

    let txn = Transaction::random(rng);

    // Give the transaction to node 1 to gossip to node 0.
    network
        .process_injected_effect_on(&node_1, announce_transaction_received(&txn))
        .await;

    // Run the network until node 1 has sent the gossip request and node 0 has handled it to the
    // point where the `NewItemBody` announcement has been received).
    let got_new_item_body_announcement = |event: &Event| -> bool {
        matches!(
            event,
            Event::TransactionGossiperAnnouncement(GossiperAnnouncement::NewItemBody { .. })
        )
    };
    network
        .crank_all_until(&node_0, rng, got_new_item_body_announcement, TIMEOUT)
        .await;

    // Run node 0 until it receives its own `CheckItemReceivedTimeout` event.
    let received_timeout_event = |event: &Event| -> bool {
        matches!(
            event,
            Event::TransactionGossiper(super::Event::CheckItemReceivedTimeout { .. })
        )
    };
    network
        .crank_until(&node_0, rng, received_timeout_event, TIMEOUT)
        .await;

    // Ensure node 0 makes a `FinishedGossiping` announcement.
    let made_finished_gossiping_announcement = |event: &Event| -> bool {
        matches!(
            event,
            Event::TransactionGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(_))
        )
    };
    network
        .crank_until(&node_0, rng, made_finished_gossiping_announcement, TIMEOUT)
        .await;

    NetworkController::<NodeMessage>::remove_active();
}

#[tokio::test]
async fn should_not_gossip_old_stored_item_again() {
    const NETWORK_SIZE: usize = 2;
    const TIMEOUT: Duration = Duration::from_secs(2);

    NetworkController::<NodeMessage>::create_active();
    let mut network = TestingNetwork::<Reactor>::new();
    let rng = &mut TestRng::new();

    let node_ids = network.add_nodes(rng, NETWORK_SIZE).await;
    let node_0 = node_ids[0];

    let txn = Transaction::random(rng);

    // Store the transaction on node 0.
    let store_txn = |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .put_transaction_to_storage(txn.clone())
            .ignore()
    };
    network.process_injected_effect_on(&node_0, store_txn).await;

    // Node 1 sends a gossip message to node 0.
    network
        .process_injected_effect_on(&node_0, |effect_builder| {
            let event = Event::TransactionGossiperIncoming(GossiperIncoming {
                sender: node_ids[1],
                message: Box::new(Message::Gossip(txn.gossip_id())),
            });
            effect_builder
                .into_inner()
                .schedule(event, QueueKind::Gossip)
                .ignore()
        })
        .await;

    // Run node 0 until it has handled the gossip message and checked if the transaction is already
    // stored.
    let checked_if_stored = |event: &Event| -> bool {
        matches!(
            event,
            Event::TransactionGossiper(super::Event::IsStoredResult { .. })
        )
    };
    network
        .crank_until(&node_0, rng, checked_if_stored, TIMEOUT)
        .await;
    // Assert the message did not cause a new entry in the gossip table and spawned no new events.
    assert!(network
        .nodes()
        .get(&node_0)
        .unwrap()
        .reactor()
        .inner()
        .transaction_gossiper
        .table
        .is_empty());
    assert!(matches!(
        network.crank(&node_0, rng).await,
        TryCrankOutcome::NoEventsToProcess
    ));

    NetworkController::<NodeMessage>::remove_active();
}

enum Unexpected {
    Response,
    GetItem,
    Item,
}

async fn should_ignore_unexpected_message(message_type: Unexpected) {
    const NETWORK_SIZE: usize = 2;
    const TIMEOUT: Duration = Duration::from_secs(2);

    NetworkController::<NodeMessage>::create_active();
    let mut network = TestingNetwork::<Reactor>::new();
    let rng = &mut TestRng::new();

    let node_ids = network.add_nodes(rng, NETWORK_SIZE).await;
    let node_0 = node_ids[0];

    let txn = Box::new(Transaction::random(rng));

    let message = match message_type {
        Unexpected::Response => Message::GossipResponse {
            item_id: txn.gossip_id(),
            is_already_held: false,
        },
        Unexpected::GetItem => Message::GetItem(txn.gossip_id()),
        Unexpected::Item => Message::Item(txn),
    };

    // Node 1 sends an unexpected message to node 0.
    network
        .process_injected_effect_on(&node_0, |effect_builder| {
            let event = Event::TransactionGossiperIncoming(GossiperIncoming {
                sender: node_ids[1],
                message: Box::new(message),
            });
            effect_builder
                .into_inner()
                .schedule(event, QueueKind::Gossip)
                .ignore()
        })
        .await;

    // Run node 0 until it has handled the gossip message.
    let received_gossip_message =
        |event: &Event| -> bool { matches!(event, Event::TransactionGossiperIncoming(..)) };
    network
        .crank_until(&node_0, rng, received_gossip_message, TIMEOUT)
        .await;
    // Assert the message did not cause a new entry in the gossip table and spawned no new events.
    assert!(network
        .nodes()
        .get(&node_0)
        .unwrap()
        .reactor()
        .inner()
        .transaction_gossiper
        .table
        .is_empty());
    assert!(matches!(
        network.crank(&node_0, rng).await,
        TryCrankOutcome::NoEventsToProcess
    ));

    NetworkController::<NodeMessage>::remove_active();
}

#[tokio::test]
async fn should_ignore_unexpected_response_message() {
    should_ignore_unexpected_message(Unexpected::Response).await
}

#[tokio::test]
async fn should_ignore_unexpected_get_item_message() {
    should_ignore_unexpected_message(Unexpected::GetItem).await
}

#[tokio::test]
async fn should_ignore_unexpected_item_message() {
    should_ignore_unexpected_message(Unexpected::Item).await
}
