//! Tests for the `small_network` component.
//!
//! Calling these "unit tests" would be a bit of a misnomer, since they deal mostly with multiple
//! instances of `small_net` arranged in a network.

use std::{
    collections::{HashMap, HashSet},
    env,
    fmt::{self, Debug, Display, Formatter},
    time::{Duration, Instant},
};

use derive_more::From;
use pnet::datalink;
use prometheus::Registry;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use super::{Config, Event as SmallNetworkEvent, GossipedAddress, SmallNetwork};
use crate::{
    components::{
        gossiper::{self, Gossiper},
        network::ENABLE_SMALL_NET_ENV_VAR,
        small_network::SmallNetworkIdentity,
        Component,
    },
    crypto::hash::Digest,
    effect::{
        announcements::{GossiperAnnouncement, NetworkAnnouncement},
        requests::{NetworkRequest, StorageRequest},
        EffectBuilder, Effects,
    },
    protocol,
    reactor::{self, EventQueueHandle, Finalize, Reactor, Runner},
    testing::{
        self, init_logging,
        network::{Network, NetworkedReactor},
        ConditionCheckReactor,
    },
    types::NodeId,
    utils::Source,
    NodeRng,
};

/// Test-reactor event.
#[derive(Debug, From, Serialize)]
enum Event {
    #[from]
    SmallNet(#[serde(skip_serializing)] SmallNetworkEvent<Message>),
    #[from]
    AddressGossiper(#[serde(skip_serializing)] gossiper::Event<GossipedAddress>),
    #[from]
    NetworkRequest(#[serde(skip_serializing)] NetworkRequest<NodeId, Message>),
    #[from]
    NetworkAnnouncement(#[serde(skip_serializing)] NetworkAnnouncement<NodeId, Message>),
    #[from]
    AddressGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<GossipedAddress>),
}

impl From<NetworkRequest<NodeId, gossiper::Message<GossipedAddress>>> for Event {
    fn from(request: NetworkRequest<NodeId, gossiper::Message<GossipedAddress>>) -> Self {
        Event::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<NodeId, protocol::Message>> for Event {
    fn from(_request: NetworkRequest<NodeId, protocol::Message>) -> Self {
        unreachable!()
    }
}

impl From<StorageRequest> for Event {
    fn from(_request: StorageRequest) -> Self {
        unreachable!()
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, From)]
enum Message {
    #[from]
    AddressGossiper(gossiper::Message<GossipedAddress>),
}

impl Display for Message {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

/// Test reactor.
///
/// Runs a single small network.
#[derive(Debug)]
struct TestReactor {
    net: SmallNetwork<Event, Message>,
    address_gossiper: Gossiper<GossipedAddress, Event>,
}

impl Reactor for TestReactor {
    type Event = Event;
    type Config = Config;
    type Error = anyhow::Error;

    fn new(
        cfg: Self::Config,
        registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut NodeRng,
    ) -> anyhow::Result<(Self, Effects<Self::Event>)> {
        let small_network_identity = SmallNetworkIdentity::new()?;
        let (net, effects) = SmallNetwork::new(
            event_queue,
            cfg,
            registry,
            small_network_identity,
            Digest::default(),
            "test-network".to_string(),
            false,
        )?;
        let gossiper_config = gossiper::Config::new_with_small_timeouts();
        let address_gossiper =
            Gossiper::new_for_complete_items("address_gossiper", gossiper_config, registry)?;

        Ok((
            TestReactor {
                net,
                address_gossiper,
            },
            reactor::wrap_effects(Event::SmallNet, effects),
        ))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::SmallNet(ev) => reactor::wrap_effects(
                Event::SmallNet,
                self.net.handle_event(effect_builder, rng, ev),
            ),
            Event::AddressGossiper(event) => reactor::wrap_effects(
                Event::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, event),
            ),
            Event::NetworkRequest(req) => self.dispatch_event(
                effect_builder,
                rng,
                Event::SmallNet(SmallNetworkEvent::from(req)),
            ),
            Event::NetworkAnnouncement(NetworkAnnouncement::MessageReceived {
                sender,
                payload,
            }) => {
                let reactor_event = match payload {
                    Message::AddressGossiper(message) => {
                        Event::AddressGossiper(gossiper::Event::MessageReceived { sender, message })
                    }
                };
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
            Event::NetworkAnnouncement(NetworkAnnouncement::GossipOurAddress(gossiped_address)) => {
                let event = gossiper::Event::ItemReceived {
                    item_id: gossiped_address,
                    source: Source::<NodeId>::Client,
                };
                self.dispatch_event(effect_builder, rng, Event::AddressGossiper(event))
            }
            Event::NetworkAnnouncement(NetworkAnnouncement::NewPeer(_)) => {
                // We do not care about the announcement of new peers in this test.
                Effects::new()
            }
            Event::AddressGossiperAnnouncement(ann) => {
                let GossiperAnnouncement::NewCompleteItem(gossiped_address) = ann;
                let reactor_event =
                    Event::SmallNet(SmallNetworkEvent::PeerAddressReceived(gossiped_address));
                self.dispatch_event(effect_builder, rng, reactor_event)
            }
        }
    }
}

impl NetworkedReactor for TestReactor {
    type NodeId = NodeId;

    fn node_id(&self) -> NodeId {
        self.net.node_id()
    }
}

impl Finalize for TestReactor {
    fn finalize(self) -> futures::future::BoxFuture<'static, ()> {
        self.net.finalize()
    }
}

/// Checks whether or not a given network with a unhealthy node is completely connected.
fn network_is_complete(
    blocklist: &HashSet<NodeId>,
    nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<TestReactor>>>,
) -> bool {
    // We need at least one node.
    if nodes.is_empty() {
        return false;
    }

    if nodes.len() == 1 {
        let nodes = &nodes.values().collect::<Vec<_>>();
        let net = &nodes[0].reactor().inner().net;
        if net.is_isolated() {
            return true;
        }
    }

    for (node_id, node) in nodes {
        let net = &node.reactor().inner().net;
        if blocklist.contains(node_id) {
            // ignore blocklisted node
            continue;
        }
        let outgoing = net.outgoing.keys().collect::<HashSet<_>>();
        let incoming = net.incoming.keys().collect::<HashSet<_>>();
        let difference = incoming
            .symmetric_difference(&outgoing)
            .collect::<HashSet<_>>();

        // All nodes should be connected to every other node, except itself, so we add it to the
        // set of nodes and pretend we have a loopback connection.
        if !difference.is_empty() {
            return false;
        }

        if outgoing.is_empty() && incoming.is_empty() {
            return false;
        }
    }
    true
}

/// Checks whether or not a given network has at least one other node in it
fn network_started(net: &Network<TestReactor>) -> bool {
    net.nodes()
        .iter()
        .map(|(_, runner)| runner.reactor().inner().net.peers())
        .all(|peers| !peers.is_empty())
}

/// Run a two-node network five times.
///
/// Ensures that network cleanup and basic networking works.
#[tokio::test]
async fn run_two_node_network_five_times() {
    // If the env var "CASPER_ENABLE_LEGACY_NET" is not defined, exit without running the test.
    if env::var(ENABLE_SMALL_NET_ENV_VAR).is_err() {
        return;
    }

    let mut rng = crate::new_rng();

    // The networking port used by the tests for the root node.
    let first_node_port = testing::unused_port_on_localhost() + 1;

    init_logging();

    for i in 0..5 {
        info!("two-network test round {}", i);

        let mut net = Network::new();

        let start = Instant::now();
        net.add_node_with_config(
            Config::default_local_net_first_node(first_node_port),
            &mut rng,
        )
        .await
        .unwrap();
        net.add_node_with_config(Config::default_local_net(first_node_port), &mut rng)
            .await
            .unwrap();
        let end = Instant::now();

        debug!(
            total_time_ms = (end - start).as_millis() as u64,
            "finished setting up networking nodes"
        );

        let timeout = Duration::from_secs(20);
        let blocklist = HashSet::new();
        net.settle_on(
            &mut rng,
            |nodes| network_is_complete(&blocklist, nodes),
            timeout,
        )
        .await;

        assert!(
            network_started(&net),
            "each node is connected to at least one other node"
        );

        let quiet_for = Duration::from_millis(25);
        let timeout = Duration::from_secs(2);
        net.settle(&mut rng, quiet_for, timeout).await;

        assert!(
            network_is_complete(&blocklist, net.nodes()),
            "network did not stay connected"
        );

        net.finalize().await;
    }
}

/// Sanity check that we can bind to a real network.
///
/// Very unlikely to ever fail on a real machine.
#[tokio::test]
async fn bind_to_real_network_interface() {
    // If the env var "CASPER_ENABLE_LEGACY_NET" is not defined, exit without running the test.
    if env::var(ENABLE_SMALL_NET_ENV_VAR).is_err() {
        return;
    }

    init_logging();

    let mut rng = crate::new_rng();

    let iface = datalink::interfaces()
        .into_iter()
        .find(|net| !net.ips.is_empty() && !net.ips.iter().any(|ip| ip.ip().is_loopback()))
        .expect("could not find a single networking interface that isn't localhost");

    let local_addr = iface
        .ips
        .into_iter()
        .next()
        .expect("found a interface with no ips")
        .ip();
    let port = testing::unused_port_on_localhost();

    let local_net_config = Config::new((local_addr, port).into());

    let mut net = Network::<TestReactor>::new();
    net.add_node_with_config(local_net_config, &mut rng)
        .await
        .unwrap();

    // The network should be fully connected.
    let timeout = Duration::from_secs(2);
    let blocklist = HashSet::new();
    net.settle_on(
        &mut rng,
        |nodes| network_is_complete(&blocklist, nodes),
        timeout,
    )
    .await;

    net.finalize().await;
}

/// Check that a network of varying sizes will connect all nodes properly.
#[tokio::test]
async fn check_varying_size_network_connects() {
    // If the env var "CASPER_ENABLE_LEGACY_NET" is not defined, exit without running the test.
    if env::var(ENABLE_SMALL_NET_ENV_VAR).is_err() {
        return;
    }

    init_logging();

    let mut rng = crate::new_rng();

    // Try with a few predefined sets of network sizes.
    for &number_of_nodes in &[2u16, 3, 5, 9, 15] {
        let timeout = Duration::from_secs(3 * number_of_nodes as u64);

        let mut net = Network::new();

        // Pick a random port in the higher ranges that is likely to be unused.
        let first_node_port = testing::unused_port_on_localhost();

        let _ = net
            .add_node_with_config(
                Config::default_local_net_first_node(first_node_port),
                &mut rng,
            )
            .await
            .unwrap();

        for _ in 1..number_of_nodes {
            net.add_node_with_config(Config::default_local_net(first_node_port), &mut rng)
                .await
                .unwrap();
        }

        // The network should be fully connected.
        let blocklist = HashSet::new();
        net.settle_on(
            &mut rng,
            |nodes| network_is_complete(&blocklist, nodes),
            timeout,
        )
        .await;

        let blocklist = HashSet::new();
        // This should not make a difference at all, but we're paranoid, so check again.
        assert!(
            network_is_complete(&blocklist, net.nodes()),
            "network did not stay connected after being settled"
        );

        // This test will run multiple times, so ensure we cleanup all ports.
        net.finalize().await;
    }
}
