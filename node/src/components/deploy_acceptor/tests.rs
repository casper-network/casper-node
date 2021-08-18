#![cfg(test)]
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, VecDeque},
    fmt::{self, Debug, Display, Formatter},
    iter,
    time::Duration,
};

use derive_more::From;
use futures::channel::oneshot;
use once_cell::sync::Lazy;
use prometheus::Registry;
use reactor::ReactorEvent;
use serde::Serialize;
use tempfile::TempDir;
use thiserror::Error;
use tracing::debug;

use casper_execution_engine::{
    core::engine_state::{BalanceResult, QueryResult, MAX_PAYMENT_AMOUNT},
    shared::{
        account::Account, stored_value::StoredValue, system_config::SystemConfig,
        wasm_config::WasmConfig,
    },
    storage::trie::merkle_proof::TrieMerkleProof,
};
use casper_types::{AccessRights, CLValue, ProtocolVersion, PublicKey, SecretKey, URef, U512};

use super::*;

use crate::{
    components::{
        contract_runtime::{self, ContractRuntime},
        gossiper::Message,
        in_memory_network::{self, InMemoryNetwork, NetworkController},
        storage::{self, Storage},
    },
    crypto::AsymmetricKeyExt,
    effect::{
        announcements::{
            ContractRuntimeAnnouncement, ControlAnnouncement, DeployAcceptorAnnouncement,
            NetworkAnnouncement, RpcServerAnnouncement,
        },
        requests::{ConsensusRequest, ContractRuntimeRequest, LinearChainRequest, NetworkRequest},
        Responder,
    },
    protocol::Message as NodeMessage,
    reactor::{self, EventQueueHandle, QueueKind, Runner},
    testing::{
        network::{Network, NetworkedReactor},
        ConditionCheckReactor,
    },
    types::{Block, Chainspec, Deploy, NodeId, Tag},
    utils::{Loadable, WithDir},
    NodeRng,
};

const VALID_PURSE_UREF: URef = URef::new([255; 32], AccessRights::READ_ADD_WRITE);
const INVALID_PURSE_UREF: URef = URef::new([254; 32], AccessRights::READ_ADD_WRITE);

static VALID_BALANCE_AMOUNT: Lazy<U512> = Lazy::new(|| U512::from(MAX_PAYMENT_AMOUNT + 1));
static INVALID_BALANCE_AMOUNT: Lazy<U512> = Lazy::new(|| U512::from(MAX_PAYMENT_AMOUNT - 1));

static VALID_ACCOUNT_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([199; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});
static INVALID_ACCOUNT_PUBLIC_KEY: Lazy<PublicKey> = Lazy::new(|| {
    let secret_key = SecretKey::ed25519_from_bytes([198; SecretKey::ED25519_LENGTH]).unwrap();
    PublicKey::from(&secret_key)
});

/// Top-level event for the reactor.
#[derive(Debug, From, Serialize)]
#[must_use]
enum Event {
    #[from]
    Network(in_memory_network::Event<NodeMessage>),
    #[from]
    Storage(#[serde(skip_serializing)] storage::Event),
    #[from]
    DeployAcceptor(#[serde(skip_serializing)] super::Event),
    #[from]
    NetworkRequest(NetworkRequest<NodeId, NodeMessage>),
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    #[from]
    NetworkAnnouncement(#[serde(skip_serializing)] NetworkAnnouncement<NodeId, NodeMessage>),
    #[from]
    RpcServerAnnouncement(#[serde(skip_serializing)] RpcServerAnnouncement),
    #[from]
    DeployAcceptorAnnouncement(#[serde(skip_serializing)] DeployAcceptorAnnouncement<NodeId>),
    #[from]
    ContractRuntime(#[serde(skip_serializing)] ContractRuntimeRequest),
}

impl ReactorEvent for Event {
    fn as_control(&self) -> Option<&ControlAnnouncement> {
        if let Self::ControlAnnouncement(ref ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }
}

impl From<StorageRequest> for Event {
    fn from(request: StorageRequest) -> Self {
        Event::Storage(storage::Event::from(request))
    }
}

impl From<NetworkRequest<NodeId, Message<Deploy>>> for Event {
    fn from(request: NetworkRequest<NodeId, Message<Deploy>>) -> Self {
        Event::NetworkRequest(request.map_payload(NodeMessage::from))
    }
}

impl From<ConsensusRequest> for Event {
    fn from(_request: ConsensusRequest) -> Self {
        unimplemented!("not implemented for acceptor tests")
    }
}

impl From<LinearChainRequest<NodeId>> for Event {
    fn from(_request: LinearChainRequest<NodeId>) -> Self {
        unimplemented!("not implemented for acceptor tests")
    }
}

impl From<ContractRuntimeAnnouncement> for Event {
    fn from(_request: ContractRuntimeAnnouncement) -> Self {
        unimplemented!("not implemented for acceptor tests")
    }
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::Network(event) => write!(formatter, "event: {}", event),
            Event::Storage(event) => write!(formatter, "storage: {}", event),
            Event::DeployAcceptor(event) => write!(formatter, "deploy acceptor: {}", event),
            Event::NetworkRequest(req) => write!(formatter, "network request: {}", req),
            Event::ControlAnnouncement(ctrl_ann) => write!(formatter, "control: {}", ctrl_ann),
            Event::NetworkAnnouncement(ann) => write!(formatter, "network announcement: {}", ann),
            Event::RpcServerAnnouncement(ann) => {
                write!(formatter, "api server announcement: {}", ann)
            }
            Event::DeployAcceptorAnnouncement(ann) => {
                write!(formatter, "deploy-acceptor announcement: {}", ann)
            }

            Event::ContractRuntime(event) => {
                write!(formatter, "contract-runtime event: {:?}", event)
            }
        }
    }
}

/// Error type returned by the test reactor.
#[derive(Debug, Error)]
enum Error {
    #[error("prometheus (metrics) error: {0}")]
    Metrics(#[from] prometheus::Error),
}

struct Reactor {
    network: InMemoryNetwork<NodeMessage>,
    storage: Storage,
    deploy_acceptor: DeployAcceptor,
    contract_runtime: ContractRuntime,
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
        _config: Self::Config,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut NodeRng,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let network = NetworkController::create_node(event_queue, rng);

        let (storage_config, storage_tempdir) = storage::Config::default_for_tests();
        let storage_withdir = WithDir::new(storage_tempdir.path(), storage_config);
        let storage = Storage::new(
            &storage_withdir,
            None,
            ProtocolVersion::from_parts(1, 0, 0),
            false,
            "test",
        )
        .unwrap();

        let contract_runtime_config = contract_runtime::Config::default();
        let contract_runtime = ContractRuntime::new(
            ProtocolVersion::from_parts(1, 0, 0),
            storage.root_path(),
            &contract_runtime_config,
            WasmConfig::default(),
            SystemConfig::default(),
            registry,
        )
        .unwrap();

        let deploy_acceptor = DeployAcceptor::new(
            super::Config::new(true),
            &Chainspec::from_resources("local"),
        );

        let reactor = Reactor {
            network,
            storage,
            deploy_acceptor,
            contract_runtime,
            _storage_tempdir: storage_tempdir,
        };

        let effects = Effects::new();

        Ok((reactor, effects))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Storage(event) => {
                info!("Storage request made");
                reactor::wrap_effects(
                    Event::Storage,
                    self.storage.handle_event(effect_builder, rng, event),
                )
            }
            Event::DeployAcceptor(event) => reactor::wrap_effects(
                Event::DeployAcceptor,
                self.deploy_acceptor
                    .handle_event(effect_builder, rng, event),
            ),
            Event::NetworkRequest(request) => reactor::wrap_effects(
                Event::Network,
                self.network
                    .handle_event(effect_builder, rng, request.into()),
            ),
            Event::ControlAnnouncement(ctrl_ann) => {
                unreachable!("unhandled control announcement: {}", ctrl_ann)
            }
            Event::NetworkAnnouncement(NetworkAnnouncement::MessageReceived {
                sender,
                payload,
            }) => {
                let reactor_event = match payload {
                    NodeMessage::GetRequest {
                        tag: Tag::Deploy,
                        serialized_id,
                    } => {
                        // Note: This is copied almost verbatim from the validator reactor and
                        // needs to be refactored.

                        let deploy_hash = match bincode::deserialize(&serialized_id) {
                            Ok(hash) => hash,
                            Err(error) => {
                                error!(
                                    "failed to decode {:?} from {}: {}",
                                    serialized_id, sender, error
                                );
                                return Effects::new();
                            }
                        };

                        match self
                            .storage
                            .handle_deduplicated_legacy_direct_deploy_request(deploy_hash)
                        {
                            Some(serialized_item) => {
                                let message = NodeMessage::new_get_response_raw_unchecked::<Deploy>(
                                    serialized_item,
                                );
                                return effect_builder.send_message(sender, message).ignore();
                            }

                            None => {
                                debug!(%sender, %deploy_hash, "failed to get deploy (not found)");
                                return Effects::new();
                            }
                        }
                    }
                    NodeMessage::GetResponse {
                        tag: Tag::Deploy,
                        serialized_item,
                    } => {
                        let deploy = match bincode::deserialize(&serialized_item) {
                            Ok(deploy) => Box::new(deploy),
                            Err(error) => {
                                error!("failed to decode deploy from {}: {}", sender, error);
                                return Effects::new();
                            }
                        };
                        Event::DeployAcceptor(super::Event::Accept {
                            deploy,
                            source: Source::Peer(sender),
                            responder: None,
                        })
                    }
                    msg => panic!("should not get {}", msg),
                };
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::NetworkAnnouncement(NetworkAnnouncement::GossipOurAddress(_)) => {
                unreachable!("should not receive announcements of type GossipOurAddress");
            }
            Event::NetworkAnnouncement(NetworkAnnouncement::NewPeer(_)) => {
                // We do not care about new peers in the acceptor test.
                Effects::new()
            }
            Event::DeployAcceptorAnnouncement(_) => {
                // We do not care about deploy acceptor announcements in the acceptor tests.
                Effects::new()
            }
            Event::RpcServerAnnouncement(RpcServerAnnouncement::DeployReceived {
                deploy,
                responder,
            }) => {
                let event = super::Event::Accept {
                    deploy,
                    source: Source::<NodeId>::Client,
                    responder,
                };
                info!("rpc server announcement made");
                self.dispatch_event(effect_builder, rng, Event::DeployAcceptor(event))
            }

            Event::Network(event) => reactor::wrap_effects(
                Event::Network,
                self.network.handle_event(effect_builder, rng, event),
            ),
            Event::ContractRuntime(event) => {
                info!("Request made to global state");
                match event {
                    ContractRuntimeRequest::Query {
                        query_request,
                        responder,
                    } => {
                        let mut effects = Effects::new();
                        // Only respond to queries for accounts
                        let result = if let Key::Account(account_hash) = query_request.key() {
                            let named_keys = BTreeMap::new();
                            let purse_uref = if account_hash
                                == INVALID_ACCOUNT_PUBLIC_KEY.to_account_hash()
                            {
                                INVALID_PURSE_UREF
                            } else if account_hash == VALID_ACCOUNT_PUBLIC_KEY.to_account_hash() {
                                VALID_PURSE_UREF
                            } else {
                                URef::default()
                            };
                            let preset_account =
                                Account::create(account_hash, named_keys, purse_uref);
                            QueryResult::Success {
                                value: Box::new(StoredValue::Account(preset_account)),
                                proofs: vec![],
                            }
                        } else {
                            QueryResult::RootNotFound
                        };
                        effects.extend(responder.respond(Ok(result)).ignore());
                        effects
                    }
                    ContractRuntimeRequest::GetBalance {
                        balance_request,
                        responder,
                    } => {
                        let mut effects = Effects::new();
                        let proof = TrieMerkleProof::new(
                            balance_request.purse_uref().into(),
                            StoredValue::CLValue(CLValue::from_t(()).expect("should get CLValue")),
                            VecDeque::new(),
                        );
                        let result = if balance_request.purse_uref() == VALID_PURSE_UREF {
                            BalanceResult::Success {
                                motes: *VALID_BALANCE_AMOUNT,
                                proof: Box::new(proof),
                            }
                        } else if balance_request.purse_uref() == INVALID_PURSE_UREF {
                            BalanceResult::Success {
                                motes: *INVALID_BALANCE_AMOUNT,
                                proof: Box::new(proof),
                            }
                        } else {
                            BalanceResult::RootNotFound
                        };
                        effects.extend(responder.respond(Ok(result)).ignore());
                        effects
                    }
                    _ => reactor::wrap_effects(
                        Event::ContractRuntime,
                        self.contract_runtime
                            .handle_event(effect_builder, rng, event),
                    ),
                }
            }
        }
    }

    fn maybe_exit(&self) -> Option<crate::reactor::ReactorExit> {
        unimplemented!()
    }
}

impl NetworkedReactor for Reactor {
    type NodeId = NodeId;

    fn node_id(&self) -> NodeId {
        self.network.node_id()
    }
}

fn announce_deploy_received(
    deploy: Box<Deploy>,
    responder: Option<Responder<Result<(), super::Error>>>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .announce_deploy_received(deploy, responder)
            .ignore()
    }
}

fn put_block_to_storage(
    block: Box<Block>,
    responder: Responder<bool>,
) -> impl FnOnce(EffectBuilder<Event>) -> Effects<Event> {
    |effect_builder: EffectBuilder<Event>| {
        effect_builder
            .into_inner()
            .schedule(
                StorageRequest::PutBlock { block, responder },
                QueueKind::Regular,
            )
            .ignore()
    }
}

async fn run_deploy_acceptor(rng: &mut NodeRng, public_key: PublicKey) -> Result<(), super::Error> {
    const TIMEOUT: Duration = Duration::from_secs(20);
    const QUIET_FOR: Duration = Duration::from_millis(50);

    let (deploy_sender, deploy_receiver) = oneshot::channel();
    let deploy_responder = Responder::create(deploy_sender);

    NetworkController::<NodeMessage>::create_active();
    let mut network = Network::<Reactor>::new();

    let node_ids = network.add_nodes(rng, 1).await;

    let deploy = Box::new(Deploy::random_with_account(rng, Some(public_key.clone())));
    let deploy_hash = *deploy.id();

    let block = Box::new(Block::random(rng));

    let (block_sender, block_receiver) = oneshot::channel();
    let block_responder = Responder::create(block_sender);
    let _block_hash = block.hash();

    network
        .process_injected_effect_on(&node_ids[0], put_block_to_storage(block, block_responder))
        .await;

    network
        .process_injected_effect_on(
            &node_ids[0],
            announce_deploy_received(deploy, Some(deploy_responder)),
        )
        .await;

    // We expect this deploy to be rejected, therefore it should not be present in storage.
    let no_such_deploy = |nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
        nodes.values().all(|runner| {
            if runner
                .reactor()
                .inner()
                .storage
                .get_deploy_by_hash(deploy_hash)
                .is_none()
            {
                true
            } else {
                false
            }
        })
    };

    network.settle_on(rng, no_such_deploy, TIMEOUT).await;

    // Ensure all responders are called before dropping the network.
    network.settle(rng, QUIET_FOR, TIMEOUT).await;

    match block_receiver.await {
        Ok(result) => assert!(result),
        Err(_) => panic!("Responder was dropped"),
    }

    let result = match deploy_receiver.await {
        Ok(result) => result,
        Err(_) => panic!("receiver error implies a bug"),
    };

    NetworkController::<NodeMessage>::remove_active();

    result
}

// Test that deploys from a peer are accepted.
#[tokio::test]
async fn should_accept_deploys_from_peer() {
    const TIMEOUT: Duration = Duration::from_secs(20);
    const QUIET_FOR: Duration = Duration::from_millis(50);

    let mut rng = crate::new_rng();

    NetworkController::<NodeMessage>::create_active();
    let mut network = Network::<Reactor>::new();

    let node_ids = network.add_nodes(&mut rng, 1).await;

    let (all_deploy_hashes, mut deploys): (BTreeSet<_>, Vec<_>) = iter::repeat_with(|| {
        let deploy = Box::new(Deploy::random(&mut rng));
        (*deploy.id(), deploy)
    })
    .take(2)
    .unzip();

    for deploy in deploys.drain(..) {
        network
            .process_injected_effect_on(&node_ids[0], announce_deploy_received(deploy, None))
            .await;
    }

    // Check every node has every deploy stored locally.
    let all_deploys_held = |nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<Reactor>>>| {
        nodes.values().all(|runner| {
            let hashes = runner.reactor().inner().storage.get_all_deploy_hashes();
            all_deploy_hashes == hashes
        })
    };
    network.settle_on(&mut rng, all_deploys_held, TIMEOUT).await;

    // Ensure all responders are called before dropping the network.
    network.settle(&mut rng, QUIET_FOR, TIMEOUT).await;

    NetworkController::<NodeMessage>::remove_active();
}

// Test deploys sent from a client are rejected due to an invalid account.
#[tokio::test]
async fn should_reject_due_to_invalid_account() {
    let mut rng = crate::new_rng();
    let public_key = PublicKey::random(&mut rng);
    let result = run_deploy_acceptor(&mut rng, public_key).await;
    assert!(matches!(result, Err(super::Error::InvalidAccount)))
}

// Test deploys sent from a client are accepted for a valid account with a minimum balance.
#[tokio::test]
async fn should_accept_deploy_for_valid_account() {
    let mut rng = crate::new_rng();
    let result = run_deploy_acceptor(&mut rng, VALID_ACCOUNT_PUBLIC_KEY.clone()).await;
    assert!(result.is_ok())
}

// Test deploys sent from a client are rejected for a valid account with a less than minimum balance.
#[tokio::test]
async fn should_reject_deploy_for_invalid_balance() {
    let mut rng = crate::new_rng();
    let result = run_deploy_acceptor(&mut rng, INVALID_ACCOUNT_PUBLIC_KEY.clone()).await;
    assert!(matches!(result, Err(super::Error::InsufficientBalance)))
}
