#![cfg(test)]

use std::{
    fmt::{self, Display, Formatter},
    sync::{Arc, Mutex},
};

use derive_more::From;
use futures::FutureExt;
use serde::Serialize;
use tempfile::TempDir;
use thiserror::Error;

use casper_types::{
    testing::TestRng, BlockV2, Chainspec, ChainspecRawBytes, FinalitySignature, Transaction,
    TransactionHash, TransactionId,
};

use super::*;
use crate::{
    components::{
        consensus::ConsensusRequestMessage,
        fetcher,
        in_memory_network::{self, InMemoryNetwork, NetworkController},
        network::{GossipedAddress, Identity as NetworkIdentity},
        storage::{self, Storage},
        transaction_acceptor,
    },
    effect::{
        announcements::{ControlAnnouncement, FatalAnnouncement, TransactionAcceptorAnnouncement},
        incoming::{
            ConsensusMessageIncoming, DemandIncoming, FinalitySignatureIncoming, GossiperIncoming,
            NetRequestIncoming, NetResponse, NetResponseIncoming, TrieDemand, TrieRequestIncoming,
            TrieResponseIncoming,
        },
        requests::{AcceptTransactionRequest, MarkBlockCompletedRequest},
    },
    fatal,
    protocol::Message,
    reactor::{self, EventQueueHandle, Reactor as ReactorTrait, ReactorEvent, Runner},
    testing::{
        self,
        network::{NetworkedReactor, TestingNetwork},
        ConditionCheckReactor, FakeTransactionAcceptor,
    },
    types::NodeId,
    utils::WithDir,
};

const TIMEOUT: Duration = Duration::from_secs(1);

/// Error type returned by the test reactor.
#[derive(Debug, Error)]
enum Error {
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),
}

impl Drop for Reactor {
    fn drop(&mut self) {
        NetworkController::<Message>::remove_node(&self.network.node_id())
    }
}

#[derive(Debug)]
pub struct FetcherTestConfig {
    fetcher_config: Config,
    storage_config: storage::Config,
    temp_dir: TempDir,
}

impl Default for FetcherTestConfig {
    fn default() -> Self {
        let (storage_config, temp_dir) = storage::Config::default_for_tests();
        FetcherTestConfig {
            fetcher_config: Default::default(),
            storage_config,
            temp_dir,
        }
    }
}

#[derive(Debug, From, Serialize)]
enum Event {
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    #[from]
    FatalAnnouncement(FatalAnnouncement),
    #[from]
    Network(in_memory_network::Event<Message>),
    #[from]
    Storage(storage::Event),
    #[from]
    FakeTransactionAcceptor(transaction_acceptor::Event),
    #[from]
    TransactionFetcher(fetcher::Event<Transaction>),
    #[from]
    NetworkRequestMessage(NetworkRequest<Message>),
    #[from]
    StorageRequest(StorageRequest),
    #[from]
    FetcherRequestTransaction(FetcherRequest<Transaction>),
    #[from]
    BlockAccumulatorRequest(BlockAccumulatorRequest),
    #[from]
    AcceptTransactionRequest(AcceptTransactionRequest),
    #[from]
    TransactionAcceptorAnnouncement(TransactionAcceptorAnnouncement),
    #[from]
    FetchedNewFinalitySignatureAnnouncement(FetchedNewFinalitySignatureAnnouncement),
    #[from]
    FetchedNewBlockAnnouncement(FetchedNewBlockAnnouncement),
    #[from]
    NetRequestIncoming(NetRequestIncoming),
    #[from]
    NetResponseIncoming(NetResponseIncoming),
    #[from]
    BlocklistAnnouncement(PeerBehaviorAnnouncement),
    #[from]
    MarkBlockCompletedRequest(MarkBlockCompletedRequest),
    #[from]
    TrieDemand(TrieDemand),
    #[from]
    ContractRuntimeRequest(ContractRuntimeRequest),
    #[from]
    GossiperIncomingTransaction(GossiperIncoming<Transaction>),
    #[from]
    GossiperIncomingBlock(GossiperIncoming<BlockV2>),
    #[from]
    GossiperIncomingFinalitySignature(GossiperIncoming<FinalitySignature>),
    #[from]
    GossiperIncomingGossipedAddress(GossiperIncoming<GossipedAddress>),
    #[from]
    TrieRequestIncoming(TrieRequestIncoming),
    #[from]
    TrieResponseIncoming(TrieResponseIncoming),
    #[from]
    ConsensusMessageIncoming(ConsensusMessageIncoming),
    #[from]
    ConsensusDemandIncoming(DemandIncoming<ConsensusRequestMessage>),
    #[from]
    FinalitySignatureIncoming(FinalitySignatureIncoming),
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

impl ReactorEvent for Event {
    fn is_control(&self) -> bool {
        matches!(self, Event::ControlAnnouncement(_))
    }

    fn try_into_control(self) -> Option<ControlAnnouncement> {
        match self {
            Event::ControlAnnouncement(ctrl_ann) => Some(ctrl_ann),
            _ => None,
        }
    }
}

struct Reactor {
    network: InMemoryNetwork<Message>,
    storage: Storage,
    fake_transaction_acceptor: FakeTransactionAcceptor,
    transaction_fetcher: Fetcher<Transaction>,
}

impl ReactorTrait for Reactor {
    type Event = Event;
    type Config = FetcherTestConfig;
    type Error = Error;

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Network(event) => reactor::wrap_effects(
                Event::Network,
                self.network.handle_event(effect_builder, rng, event),
            ),
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::FakeTransactionAcceptor(event) => reactor::wrap_effects(
                Event::FakeTransactionAcceptor,
                self.fake_transaction_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            Event::TransactionFetcher(event) => reactor::wrap_effects(
                Event::TransactionFetcher,
                self.transaction_fetcher
                    .handle_event(effect_builder, rng, event),
            ),
            Event::NetworkRequestMessage(request) => reactor::wrap_effects(
                Event::Network,
                self.network
                    .handle_event(effect_builder, rng, request.into()),
            ),
            Event::StorageRequest(request) => reactor::wrap_effects(
                Event::Storage,
                self.storage
                    .handle_event(effect_builder, rng, request.into()),
            ),
            Event::FetcherRequestTransaction(request) => reactor::wrap_effects(
                Event::TransactionFetcher,
                self.transaction_fetcher
                    .handle_event(effect_builder, rng, request.into()),
            ),
            Event::TransactionAcceptorAnnouncement(announcement) => {
                let event = fetcher::Event::from(announcement);
                reactor::wrap_effects(
                    Event::TransactionFetcher,
                    self.transaction_fetcher
                        .handle_event(effect_builder, rng, event),
                )
            }
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
                reactor::wrap_effects(
                    Event::FakeTransactionAcceptor,
                    self.fake_transaction_acceptor
                        .handle_event(effect_builder, rng, event),
                )
            }
            Event::NetRequestIncoming(announcement) => reactor::wrap_effects(
                Event::Storage,
                self.storage
                    .handle_event(effect_builder, rng, announcement.into()),
            ),
            Event::NetResponseIncoming(announcement) => {
                let mut announcement_effects = Effects::new();
                let effects = self.handle_net_response(effect_builder, rng, announcement);
                announcement_effects.extend(effects.into_iter());
                announcement_effects
            }
            Event::MarkBlockCompletedRequest(request) => reactor::wrap_effects(
                Event::Storage,
                self.storage
                    .handle_event(effect_builder, rng, request.into()),
            ),
            Event::TrieDemand(_)
            | Event::ContractRuntimeRequest(_)
            | Event::BlockAccumulatorRequest(_)
            | Event::BlocklistAnnouncement(_)
            | Event::GossiperIncomingTransaction(_)
            | Event::GossiperIncomingBlock(_)
            | Event::GossiperIncomingFinalitySignature(_)
            | Event::GossiperIncomingGossipedAddress(_)
            | Event::TrieRequestIncoming(_)
            | Event::TrieResponseIncoming(_)
            | Event::ConsensusMessageIncoming(_)
            | Event::ConsensusDemandIncoming(_)
            | Event::FinalitySignatureIncoming(_)
            | Event::FetchedNewBlockAnnouncement(_)
            | Event::FetchedNewFinalitySignatureAnnouncement(_)
            | Event::ControlAnnouncement(_)
            | Event::FatalAnnouncement(_) => panic!("unexpected: {}", event),
        }
    }

    fn new(
        cfg: Self::Config,
        chainspec: Arc<Chainspec>,
        _chainspec_raw_bytes: Arc<ChainspecRawBytes>,
        _network_identity: NetworkIdentity,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let network = InMemoryNetwork::<Message>::new(event_queue, rng);

        let storage = Storage::new(
            &WithDir::new(cfg.temp_dir.path(), cfg.storage_config),
            chainspec.hard_reset_to_start_of_era(),
            chainspec.protocol_config.version,
            chainspec.protocol_config.activation_point.era_id(),
            &chainspec.network_config.name,
            chainspec.transaction_config.max_ttl.into(),
            chainspec.core_config.unbonding_delay,
            Some(registry),
            false,
        )
        .unwrap();

        let fake_transaction_acceptor = FakeTransactionAcceptor::new();
        let transaction_fetcher =
            Fetcher::<Transaction>::new("transaction", &cfg.fetcher_config, registry).unwrap();
        let reactor = Reactor {
            network,
            storage,
            fake_transaction_acceptor,
            transaction_fetcher,
        };
        Ok((reactor, Effects::new()))
    }
}

impl Reactor {
    fn handle_net_response(
        &mut self,
        effect_builder: EffectBuilder<Event>,
        rng: &mut NodeRng,
        response: NetResponseIncoming,
    ) -> Effects<Event> {
        match *response.message {
            NetResponse::Transaction(ref serialized_item) => {
                let transaction = match bincode::deserialize::<
                    FetchResponse<Transaction, TransactionHash>,
                >(serialized_item)
                {
                    Ok(FetchResponse::Fetched(txn)) => txn,
                    Ok(FetchResponse::NotFound(txn_hash)) => {
                        return fatal!(
                            effect_builder,
                            "peer did not have transaction with hash {}: {}",
                            txn_hash,
                            response.sender,
                        )
                        .ignore();
                    }
                    Ok(FetchResponse::NotProvided(txn_hash)) => {
                        return fatal!(
                            effect_builder,
                            "peer refused to provide transaction with hash {}: {}",
                            txn_hash,
                            response.sender,
                        )
                        .ignore();
                    }
                    Err(error) => {
                        return fatal!(
                            effect_builder,
                            "failed to decode transaction from {}: {}",
                            response.sender,
                            error
                        )
                        .ignore();
                    }
                };

                self.dispatch_event(
                    effect_builder,
                    rng,
                    Event::FakeTransactionAcceptor(transaction_acceptor::Event::Accept {
                        transaction,
                        source: Source::Peer(response.sender),
                        maybe_responder: None,
                    }),
                )
            }
            _ => fatal!(
                effect_builder,
                "no support for anything but transaction responses in fetcher test"
            )
            .ignore(),
        }
    }
}

impl NetworkedReactor for Reactor {
    fn node_id(&self) -> NodeId {
        self.network.node_id()
    }
}

fn announce_transaction_received(
    txn: Transaction,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder: EffectBuilder<Event>| effect_builder.try_accept_transaction(txn, None).ignore()
}

type FetchedTransactionResult = Arc<Mutex<(bool, Option<FetchResult<Transaction>>)>>;

fn fetch_txn(
    txn_id: TransactionId,
    node_id: NodeId,
    fetched: FetchedTransactionResult,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    move |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .fetch::<Transaction>(txn_id, node_id, Box::new(EmptyValidationMetadata))
            .then(move |txn| async move {
                let mut result = fetched.lock().unwrap();
                result.0 = true;
                result.1 = Some(txn);
            })
            .ignore()
    }
}

/// Store a transaction on a target node.
async fn store_txn(
    txn: &Transaction,
    node_id: &NodeId,
    network: &mut TestingNetwork<Reactor>,
    rng: &mut TestRng,
) {
    network
        .process_injected_effect_on(node_id, announce_transaction_received(txn.clone()))
        .await;

    // cycle to transaction acceptor announcement
    network
        .crank_until(
            node_id,
            rng,
            move |event: &Event| {
                matches!(
                    event,
                    Event::TransactionAcceptorAnnouncement(
                        TransactionAcceptorAnnouncement::AcceptedNewTransaction { .. },
                    )
                )
            },
            TIMEOUT,
        )
        .await;
}

#[derive(Debug)]
enum ExpectedFetchedTransactionResult {
    TimedOut,
    FromStorage {
        expected_txn: Box<Transaction>,
    },
    FromPeer {
        expected_txn: Box<Transaction>,
        expected_peer: NodeId,
    },
}

async fn assert_settled(
    node_id: &NodeId,
    txn_id: TransactionId,
    expected_result: ExpectedFetchedTransactionResult,
    fetched: FetchedTransactionResult,
    network: &mut TestingNetwork<Reactor>,
    rng: &mut TestRng,
    timeout: Duration,
) {
    let has_responded = |_nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
        fetched.lock().unwrap().0
    };

    network.settle_on(rng, has_responded, timeout).await;

    let maybe_stored_txn = network
        .nodes()
        .get(node_id)
        .unwrap()
        .reactor()
        .inner()
        .storage
        .get_transaction_by_hash(txn_id.transaction_hash());

    let actual_fetcher_result = fetched.lock().unwrap().1.clone();
    match (expected_result, actual_fetcher_result, maybe_stored_txn) {
        // Timed-out case: despite the delayed response causing a timeout, the response does arrive,
        // and the TestTransactionAcceptor unconditionally accepts the txn and stores it.  For the
        // test, we don't care whether it was stored or not, just that the TimedOut event fired.
        (
            ExpectedFetchedTransactionResult::TimedOut,
            Some(Err(fetcher::Error::TimedOut { .. })),
            _,
        ) => {}
        // FromStorage case: expect txn to correspond to item fetched, as well as stored item.
        (
            ExpectedFetchedTransactionResult::FromStorage { expected_txn },
            Some(Ok(FetchedData::FromStorage { item })),
            Some(stored_txn),
        ) if expected_txn == item && stored_txn == *item => {}
        // FromPeer case: txns should correspond, storage should be present and correspond, and
        // peers should correspond.
        (
            ExpectedFetchedTransactionResult::FromPeer {
                expected_txn,
                expected_peer,
            },
            Some(Ok(FetchedData::FromPeer { item, peer })),
            Some(stored_txn),
        ) if expected_txn == item && stored_txn == *item && expected_peer == peer => {}
        // Sad path case
        (expected_result, actual_fetcher_result, maybe_stored_txn) => {
            panic!(
                "Expected result type {:?} but found {:?} (stored transaction is {:?})",
                expected_result, actual_fetcher_result, maybe_stored_txn
            )
        }
    }
}

#[tokio::test]
async fn should_fetch_from_local() {
    const NETWORK_SIZE: usize = 1;

    NetworkController::<Message>::create_active();
    let (mut network, mut rng, node_ids) = {
        let mut network = TestingNetwork::<Reactor>::new();
        let mut rng = TestRng::new();
        let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;
        (network, rng, node_ids)
    };

    // Create a random txn.
    let txn = Transaction::random(&mut rng);

    // Store txn on a node.
    let node_to_store_on = &node_ids[0];
    store_txn(&txn, node_to_store_on, &mut network, &mut rng).await;

    // Try to fetch the txn from a node that holds it.
    let node_id = node_ids[0];
    let txn_id = txn.fetch_id();
    let fetched = Arc::new(Mutex::new((false, None)));
    network
        .process_injected_effect_on(&node_id, fetch_txn(txn_id, node_id, Arc::clone(&fetched)))
        .await;

    let expected_result = ExpectedFetchedTransactionResult::FromStorage {
        expected_txn: Box::new(txn),
    };
    assert_settled(
        &node_id,
        txn_id,
        expected_result,
        fetched,
        &mut network,
        &mut rng,
        TIMEOUT,
    )
    .await;

    NetworkController::<Message>::remove_active();
}

#[tokio::test]
async fn should_fetch_from_peer() {
    const NETWORK_SIZE: usize = 2;

    NetworkController::<Message>::create_active();
    let (mut network, mut rng, node_ids) = {
        let mut network = TestingNetwork::<Reactor>::new();
        let mut rng = TestRng::new();
        let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;
        (network, rng, node_ids)
    };

    // Create a random txn.
    let txn = Transaction::random(&mut rng);

    // Store txn on a node.
    let node_with_txn = node_ids[0];
    store_txn(&txn, &node_with_txn, &mut network, &mut rng).await;

    let node_without_txn = node_ids[1];
    let txn_id = txn.fetch_id();
    let fetched = Arc::new(Mutex::new((false, None)));

    // Try to fetch the txn from a node that does not hold it; should get from peer.
    network
        .process_injected_effect_on(
            &node_without_txn,
            fetch_txn(txn_id, node_with_txn, Arc::clone(&fetched)),
        )
        .await;

    let expected_result = ExpectedFetchedTransactionResult::FromPeer {
        expected_txn: Box::new(txn),
        expected_peer: node_with_txn,
    };
    assert_settled(
        &node_without_txn,
        txn_id,
        expected_result,
        fetched,
        &mut network,
        &mut rng,
        TIMEOUT,
    )
    .await;

    NetworkController::<Message>::remove_active();
}

#[tokio::test]
async fn should_timeout_fetch_from_peer() {
    const NETWORK_SIZE: usize = 2;

    NetworkController::<Message>::create_active();
    let (mut network, mut rng, node_ids) = {
        let mut network = TestingNetwork::<Reactor>::new();
        let mut rng = TestRng::new();
        let node_ids = network.add_nodes(&mut rng, NETWORK_SIZE).await;
        (network, rng, node_ids)
    };

    // Create a random txn.
    let txn = Transaction::random(&mut rng);
    let txn_id = txn.fetch_id();

    let holding_node = node_ids[0];
    let requesting_node = node_ids[1];

    // Store txn on holding node.
    store_txn(&txn, &holding_node, &mut network, &mut rng).await;

    // Initiate requesting node asking for txn from holding node.
    let fetched = Arc::new(Mutex::new((false, None)));
    network
        .process_injected_effect_on(
            &requesting_node,
            fetch_txn(txn_id, holding_node, Arc::clone(&fetched)),
        )
        .await;

    // Crank until message sent from the requester.
    network
        .crank_until(
            &requesting_node,
            &mut rng,
            move |event: &Event| {
                if let Event::NetworkRequestMessage(NetworkRequest::SendMessage {
                    payload, ..
                }) = event
                {
                    matches!(**payload, Message::GetRequest { .. })
                } else {
                    false
                }
            },
            TIMEOUT,
        )
        .await;

    // Crank until the message is received by the holding node.
    network
        .crank_until(
            &holding_node,
            &mut rng,
            move |event: &Event| {
                if let Event::NetworkRequestMessage(NetworkRequest::SendMessage {
                    payload, ..
                }) = event
                {
                    matches!(**payload, Message::GetResponse { .. })
                } else {
                    false
                }
            },
            TIMEOUT,
        )
        .await;

    // Advance time.
    let duration_to_advance: Duration = Config::default().get_from_peer_timeout().into();
    let duration_to_advance = duration_to_advance + Duration::from_secs(10);
    testing::advance_time(duration_to_advance).await;

    // Settle the network, allowing timeout to avoid panic.
    let expected_result = ExpectedFetchedTransactionResult::TimedOut;
    assert_settled(
        &requesting_node,
        txn_id,
        expected_result,
        fetched,
        &mut network,
        &mut rng,
        TIMEOUT,
    )
    .await;

    NetworkController::<Message>::remove_active();
}
