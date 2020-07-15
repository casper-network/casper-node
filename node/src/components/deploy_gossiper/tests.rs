#![cfg(test)]
use std::{
    collections::{BTreeSet, HashMap},
    fmt::{self, Debug, Display, Formatter},
    iter,
};

use derive_more::From;
use tempfile::TempDir;
use tokio::time;
use tracing::debug;

use super::*;
use crate::{
    components::{
        in_memory_network::{InMemoryNetwork, NetworkController, NodeId},
        storage::{self, Storage, StorageType},
    },
    effect::announcements::NetworkAnnouncement,
    reactor::{self, EventQueueHandle, Reactor as ReactorTrait, Runner},
    testing::network::{Network, NetworkedReactor},
    types::Deploy,
};

/// Top-level event for the reactor.
#[derive(Debug, From)]
#[must_use]
enum Event {
    #[from]
    /// Storage event.
    Storage(StorageRequest<Storage>),
    /// Deploy gossiper event.
    #[from]
    DeployGossiper(super::Event),
    /// Network request.
    #[from]
    NetworkRequest(NetworkRequest<NodeId, Message>),
    /// Network announcement.
    #[from]
    NetworkAnnouncement(NetworkAnnouncement<NodeId, Message>),
}

impl Display for Event {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, formatter)
    }
}

#[derive(Debug)]
struct Reactor {
    network: InMemoryNetwork<Message>,
    storage: Storage,
    deploy_gossiper: DeployGossiper,
    _storage_tempdir: TempDir,
}

impl Drop for Reactor {
    fn drop(&mut self) {
        NetworkController::<Message>::remove_node(&self.network.node_id())
    }
}

impl ReactorTrait for Reactor {
    type Event = Event;
    type Config = GossipTableConfig;
    type Error = storage::Error;

    fn new<R: Rng + ?Sized>(
        config: Self::Config,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut R,
    ) -> Result<(Self, Effects<Self::Event>), Self::Error> {
        let network = NetworkController::create_node(event_queue, rng);

        let (storage_config, _storage_tempdir) = storage::Config::default_for_tests();
        let storage = Storage::new(&storage_config)?;

        let deploy_gossiper = DeployGossiper::new(config);

        let reactor = Reactor {
            network,
            storage,
            deploy_gossiper,
            _storage_tempdir,
        };

        let effects = Effects::new();

        Ok((reactor, effects))
    }

    fn dispatch_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut R,
        event: Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Storage(event) => reactor::wrap_effects(
                Event::Storage,
                self.storage.handle_event(effect_builder, rng, event),
            ),
            Event::DeployGossiper(event) => reactor::wrap_effects(
                Event::DeployGossiper,
                self.deploy_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            Event::NetworkRequest(request) => reactor::wrap_effects(
                Event::NetworkRequest,
                self.network.handle_event(effect_builder, rng, request),
            ),
            Event::NetworkAnnouncement(NetworkAnnouncement::MessageReceived {
                sender,
                payload,
            }) => {
                let event = super::Event::MessageReceived {
                    sender,
                    message: payload,
                };
                reactor::wrap_effects(
                    From::from,
                    self.deploy_gossiper
                        .handle_event(effect_builder, rng, event),
                )
            }
        }
    }
}

impl NetworkedReactor for Reactor {
    type NodeId = NodeId;

    fn node_id(&self) -> NodeId {
        self.network.node_id()
    }
}

#[tokio::test]
async fn should_gossip() {
    const NETWORK_SIZE_MIN: usize = 2;
    const NETWORK_SIZE_MAX: usize = 20;
    const DEPLOY_COUNT_MIN: usize = 1;
    const DEPLOY_COUNT_MAX: usize = 30;
    const TIMEOUT: Duration = Duration::from_secs(20);

    NetworkController::<Message>::create_active();
    let mut network = Network::<Reactor>::new();
    let mut rng = rand::thread_rng();

    // Add `network_size` nodes.
    let network_size: usize = rng.gen_range(NETWORK_SIZE_MIN, NETWORK_SIZE_MAX + 1);
    let mut node_ids = vec![];
    for _ in 0..network_size {
        let (node_id, _runner) = network.add_node(&mut rng).await.unwrap();
        node_ids.push(node_id);
    }

    // Create `deploy_count` random deploys.
    let deploy_count: usize = rng.gen_range(DEPLOY_COUNT_MIN, DEPLOY_COUNT_MAX + 1);
    let (all_deploy_hashes, mut deploys): (BTreeSet<_>, Vec<_>) = iter::repeat_with(|| {
        let deploy = Box::new(rng.gen::<Deploy>());
        (*deploy.id(), deploy)
    })
    .take(deploy_count)
    .unzip();

    // Give each deploy to a randomly-chosen node to be gossiped.
    for deploy in deploys.drain(..) {
        let event = Event::DeployGossiper(super::Event::DeployReceived { deploy });
        let index: usize = rng.gen_range(0, network_size);
        network
            .inject_event_on(&node_ids[index], &mut rng, event)
            .await;
    }

    // Check every node has every deploy stored locally.
    let all_deploys_held = |nodes: &HashMap<NodeId, Runner<Reactor>>| {
        nodes.values().all(|runner| {
            let hashes = runner
                .reactor()
                .storage
                .deploy_store()
                .ids()
                .unwrap()
                .into_iter()
                .collect();
            all_deploy_hashes == hashes
        })
    };
    assert!(network.settle_on(&mut rng, all_deploys_held, TIMEOUT).await);

    NetworkController::<Message>::remove_active();
}

#[tokio::test]
async fn should_get_from_alternate_source() {
    const NETWORK_SIZE: usize = 3;
    const POLL_DURATION: Duration = Duration::from_millis(10);
    const TIMEOUT: Duration = Duration::from_secs(2);

    NetworkController::<Message>::create_active();
    let mut network = Network::<Reactor>::new();
    let mut rng = rand::thread_rng();

    // Add `NETWORK_SIZE` nodes.
    let mut node_ids = vec![];
    for _ in 0..NETWORK_SIZE {
        let (node_id, _runner) = network.add_node(&mut rng).await.unwrap();
        node_ids.push(node_id);
    }

    // Create random deploy.
    let deploy = Box::new(rng.gen::<Deploy>());
    let deploy_id = *deploy.id();

    // Give the deploy to nodes 0 and 1 to be gossiped.
    for node_id in node_ids.iter().take(2) {
        let event = Event::DeployGossiper(super::Event::DeployReceived {
            deploy: deploy.clone(),
        });
        network.inject_event_on(&node_id, &mut rng, event).await;
    }

    // Run node 0 until it has sent the gossip request then remove it from the network.  This
    // equates to three events:
    // 1. Storage PutDeploy
    // 2. DeployGossiper PutToStoreResult
    // 3. NetworkRequest Gossip
    let mut event_count = 0;
    while event_count < 3 {
        event_count += network.crank(&node_ids[0], &mut rng).await;
        time::delay_for(POLL_DURATION).await;
    }
    assert!(network.remove_node(&node_ids[0]).is_some());
    debug!("removed node {}", &node_ids[0]);

    // Run node 2 until it receives the gossip request from node 0.  This equates to two events:
    // 1. NetworkAnnouncement MessageReceived of node 0's gossip message
    // 2. NetworkRequest SendMessage gossip response.
    event_count = 0;
    while event_count < 2 {
        event_count += network.crank(&node_ids[2], &mut rng).await;
        time::delay_for(POLL_DURATION).await;
    }

    // Run nodes 1 and 2 until settled.  Node 2 will be waiting for the deploy from node 0.
    assert!(network.settle(&mut rng, POLL_DURATION, TIMEOUT).await);

    // Advance time to trigger node 2's timeout causing it to request the deploy from node 1.
    let secs_to_advance = GossipTableConfig::default().get_remainder_timeout_secs();
    time::pause();
    time::advance(Duration::from_secs(secs_to_advance)).await;
    time::resume();
    debug!("advanced time by {} secs", secs_to_advance);

    // Check node 0 has the deploy stored locally.
    let deploy_held = |nodes: &HashMap<NodeId, Runner<Reactor>>| {
        let runner = nodes.get(&node_ids[2]).unwrap();
        runner
            .reactor()
            .storage
            .deploy_store()
            .get(&deploy_id)
            .map(|retrieved_deploy| retrieved_deploy == *deploy)
            .unwrap_or_default()
    };
    assert!(network.settle_on(&mut rng, deploy_held, TIMEOUT).await);

    NetworkController::<Message>::remove_active();
}

#[tokio::test]
async fn should_timeout_gossip_response() {
    const POLL_DURATION: Duration = Duration::from_millis(10);
    const TIMEOUT: Duration = Duration::from_secs(2);

    NetworkController::<Message>::create_active();
    let mut network = Network::<Reactor>::new();
    let mut rng = rand::thread_rng();

    // The target number of peers to infect with a given piece of data.
    let infection_target = GossipTableConfig::default().infection_target();

    // Add `infection_target + 1` nodes.
    let mut node_ids = vec![];
    for _ in 0..(infection_target + 1) {
        let (node_id, _runner) = network.add_node(&mut rng).await.unwrap();
        node_ids.push(node_id);
    }

    // Create random deploy.
    let deploy = Box::new(rng.gen::<Deploy>());
    let deploy_id = *deploy.id();

    // Give the deploy to node 0 to be gossiped.
    let event = Event::DeployGossiper(super::Event::DeployReceived {
        deploy: deploy.clone(),
    });
    network.inject_event_on(&node_ids[0], &mut rng, event).await;

    // Run node 0 until it has sent the gossip requests then remove it from the network.  This
    // equates to four events:
    // 1. Storage PutDeploy
    // 2. DeployGossiper PutToStoreResult
    // 3. NetworkRequest Gossip
    // 4. DeployGossiper GossipedTo
    let mut event_count = 0;
    while event_count < 4 {
        event_count += network.crank(&node_ids[0], &mut rng).await;
        time::delay_for(POLL_DURATION).await;
    }

    // Replace all nodes except node 0 with new nodes.
    for node_id in node_ids.drain(1..) {
        assert!(network.remove_node(&node_id).is_some());
        debug!("removed node {}", node_id);
    }
    for _ in 0..infection_target {
        let (node_id, _runner) = network.add_node(&mut rng).await.unwrap();
        node_ids.push(node_id);
    }

    // Advance time to trigger node 0's timeout causing it to gossip to the new nodes.
    let secs_to_advance = GossipTableConfig::default().gossip_request_timeout_secs();
    time::pause();
    time::advance(Duration::from_secs(secs_to_advance)).await;
    time::resume();
    debug!("advanced time by {} secs", secs_to_advance);

    // Check every node has every deploy stored locally.
    let deploy_held = |nodes: &HashMap<NodeId, Runner<Reactor>>| {
        nodes.values().all(|runner| {
            runner
                .reactor()
                .storage
                .deploy_store()
                .get(&deploy_id)
                .map(|retrieved_deploy| retrieved_deploy == *deploy)
                .unwrap_or_default()
        })
    };
    assert!(network.settle_on(&mut rng, deploy_held, TIMEOUT).await);

    NetworkController::<Message>::remove_active();
}
