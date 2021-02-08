use std::{
    collections::{HashMap, HashSet},
    env,
    fmt::{self, Debug, Display, Formatter},
    time::{Duration, Instant},
};

use derive_more::From;
use pnet::datalink;
use prometheus::Registry;
use serde::Serialize;
use tracing::{debug, info};

use super::{
    network_is_isolated, Config, Event as NetworkEvent, Network as NetworkComponent,
    ENABLE_LIBP2P_NET_ENV_VAR,
};
use crate::{
    components::{network::NetworkIdentity, Component},
    effect::{
        announcements::NetworkAnnouncement, requests::NetworkRequest, EffectBuilder, Effects,
    },
    protocol,
    reactor::{self, EventQueueHandle, Finalize, Reactor, Runner},
    testing::{
        self, init_logging,
        network::{Network, NetworkedReactor},
        ConditionCheckReactor,
    },
    types::{Chainspec, NodeId},
    NodeRng,
};

/// Test-reactor event.
#[derive(Debug, From, Serialize)]
enum Event {
    #[from]
    Network(#[serde(skip_serializing)] NetworkEvent<String>),
    #[from]
    NetworkRequest(#[serde(skip_serializing)] NetworkRequest<NodeId, String>),
    #[from]
    NetworkAnnouncement(#[serde(skip_serializing)] NetworkAnnouncement<NodeId, String>),
}

impl From<NetworkRequest<NodeId, protocol::Message>> for Event {
    fn from(_request: NetworkRequest<NodeId, protocol::Message>) -> Self {
        unreachable!()
    }
}

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

/// Test reactor.
///
/// Runs a single network.
#[derive(Debug)]
struct TestReactor {
    network_component: NetworkComponent<Event, String>,
}

impl Reactor for TestReactor {
    type Event = Event;
    type Config = Config;
    type Error = anyhow::Error;

    fn new(
        config: Self::Config,
        _registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        rng: &mut NodeRng,
    ) -> anyhow::Result<(Self, Effects<Self::Event>)> {
        let chainspec = Chainspec::random(rng);
        let network_identity = NetworkIdentity::new();
        let (network_component, effects) =
            NetworkComponent::new(event_queue, config, network_identity, &chainspec, false)?;

        Ok((
            TestReactor { network_component },
            reactor::wrap_effects(Event::Network, effects),
        ))
    }

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut NodeRng,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::Network(event) => reactor::wrap_effects(
                Event::Network,
                self.network_component
                    .handle_event(effect_builder, rng, event),
            ),
            Event::NetworkRequest(request) => self.dispatch_event(
                effect_builder,
                rng,
                Event::Network(NetworkEvent::from(request)),
            ),
            Event::NetworkAnnouncement(NetworkAnnouncement::MessageReceived {
                sender,
                payload,
            }) => {
                todo!("{} -- {}", sender, payload);
            }
            Event::NetworkAnnouncement(NetworkAnnouncement::GossipOurAddress(
                _gossiped_address,
            )) => {
                unreachable!();
            }
            Event::NetworkAnnouncement(NetworkAnnouncement::NewPeer(_)) => {
                // We do not care about the announcement of new peers in this test.
                Effects::new()
            }
        }
    }
}

impl NetworkedReactor for TestReactor {
    type NodeId = NodeId;

    fn node_id(&self) -> NodeId {
        self.network_component.node_id()
    }
}

impl Finalize for TestReactor {
    fn finalize(self) -> futures::future::BoxFuture<'static, ()> {
        self.network_component.finalize()
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
        let network_component = &nodes[0].reactor().inner().network_component;
        if network_is_isolated(
            &*network_component
                .known_addresses_mut
                .lock()
                .expect("Could not lock known_addresses_mut"),
        ) {
            return true;
        }
    }

    for (node_id, node) in nodes {
        if blocklist.contains(node_id) {
            // ignore blocklisted node
            continue;
        }
        if node.reactor().inner().network_component.peers.is_empty() {
            return false;
        }
    }

    true
}

/// Checks whether or not a given network has at least one other node in it
fn network_started(net: &Network<TestReactor>) -> bool {
    net.nodes()
        .iter()
        .all(|(_, runner)| !runner.reactor().inner().network_component.peers.is_empty())
}

/// Run a two-node network five times.
///
/// Ensures that network cleanup and basic networking works.
#[tokio::test]
async fn run_two_node_network_five_times() {
    // If the env var "CASPER_ENABLE_LIBP2P_NET" is defined, exit without running the test.
    if env::var(ENABLE_LIBP2P_NET_ENV_VAR).is_err() {
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
    // If the env var "CASPER_ENABLE_LIBP2P_NET" is defined, exit without running the test.
    if env::var(ENABLE_LIBP2P_NET_ENV_VAR).is_err() {
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

    let local_net_config = Config::new((local_addr, port).into(), true);

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
    // If the env var "CASPER_ENABLE_LIBP2P_NET" is defined, exit without running the test.
    if env::var(ENABLE_LIBP2P_NET_ENV_VAR).is_err() {
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
