//! Tests for the `small_network` component.
//!
//! Calling these "unit tests" would be a bit of a misnomer, since they deal mostly with multiple
//! instances of `small_net` arranged in a network.

use std::{
    collections::{HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    time::{Duration, Instant},
};

use derive_more::From;
use prometheus::Registry;
use reactor::ReactorEvent;
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use super::{
    chain_info::ChainInfo, Config, Event as SmallNetworkEvent, FromIncoming, GossipedAddress,
    MessageKind, Payload, SmallNetwork,
};
use crate::{
    components::{
        gossiper::{self, Gossiper},
        small_network::SmallNetworkIdentity,
        Component,
    },
    effect::{
        announcements::{ControlAnnouncement, GossiperAnnouncement},
        incoming::GossiperIncoming,
        requests::{
            BeginGossipRequest, ChainspecLoaderRequest, ContractRuntimeRequest, NetworkRequest,
            StorageRequest,
        },
        EffectBuilder, Effects,
    },
    protocol,
    reactor::{self, EventQueueHandle, Finalize, Reactor, Runner},
    testing::{
        self, init_logging,
        network::{Network, NetworkedReactor, Nodes},
        ConditionCheckReactor,
    },
    types::NodeId,
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
    NetworkRequest(#[serde(skip_serializing)] NetworkRequest<Message>),
    #[from]
    ControlAnnouncement(ControlAnnouncement),
    #[from]
    AddressGossiperAnnouncement(#[serde(skip_serializing)] GossiperAnnouncement<GossipedAddress>),
    #[from]
    BeginAddressGossipRequest(BeginGossipRequest<GossipedAddress>),
    /// An incoming network message with an address gossiper protocol message.
    AddressGossiperIncoming(GossiperIncoming<GossipedAddress>),
}

impl ReactorEvent for Event {
    fn as_control(&self) -> Option<&ControlAnnouncement> {
        if let Self::ControlAnnouncement(ref ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }

    fn try_into_control(self) -> Option<ControlAnnouncement> {
        if let Self::ControlAnnouncement(ctrl_ann) = self {
            Some(ctrl_ann)
        } else {
            None
        }
    }
}

impl From<NetworkRequest<gossiper::Message<GossipedAddress>>> for Event {
    fn from(request: NetworkRequest<gossiper::Message<GossipedAddress>>) -> Self {
        Event::NetworkRequest(request.map_payload(Message::from))
    }
}

impl From<NetworkRequest<Message>> for SmallNetworkEvent<Message> {
    fn from(request: NetworkRequest<Message>) -> SmallNetworkEvent<Message> {
        SmallNetworkEvent::NetworkRequest {
            req: Box::new(request),
        }
    }
}

impl From<NetworkRequest<protocol::Message>> for Event {
    fn from(_request: NetworkRequest<protocol::Message>) -> Self {
        unreachable!()
    }
}

impl From<StorageRequest> for Event {
    fn from(_request: StorageRequest) -> Self {
        unreachable!()
    }
}

impl From<ChainspecLoaderRequest> for Event {
    fn from(_request: ChainspecLoaderRequest) -> Self {
        unreachable!()
    }
}

impl From<ContractRuntimeRequest> for Event {
    fn from(_request: ContractRuntimeRequest) -> Self {
        unreachable!()
    }
}

impl FromIncoming<Message> for Event {
    fn from_incoming(sender: NodeId, payload: Message) -> Self {
        match payload {
            Message::AddressGossiper(message) => {
                Event::AddressGossiperIncoming(GossiperIncoming { sender, message })
            }
        }
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

impl Payload for Message {
    #[inline]
    fn classify(&self) -> MessageKind {
        match self {
            Message::AddressGossiper(_) => MessageKind::AddressGossip,
        }
    }

    fn incoming_resource_estimate(&self, _weights: &super::EstimatorWeights) -> u32 {
        0
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
            None,
            registry,
            small_network_identity,
            ChainInfo::create_for_testing(),
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
            Event::NetworkRequest(req) => reactor::wrap_effects(
                Event::SmallNet,
                self.net.handle_event(effect_builder, rng, req.into()),
            ),
            Event::ControlAnnouncement(ctrl_ann) => {
                unreachable!("unhandled control announcement: {}", ctrl_ann)
            }
            Event::AddressGossiperAnnouncement(GossiperAnnouncement::NewCompleteItem(
                gossiped_address,
            )) => reactor::wrap_effects(
                Event::SmallNet,
                self.net.handle_event(
                    effect_builder,
                    rng,
                    SmallNetworkEvent::PeerAddressReceived(gossiped_address),
                ),
            ),

            Event::AddressGossiperAnnouncement(GossiperAnnouncement::FinishedGossiping(_)) => {
                // We do not care about the announcement of gossiping finished in this test.
                Effects::new()
            }
            Event::BeginAddressGossipRequest(ev) => reactor::wrap_effects(
                Event::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, ev.into()),
            ),
            Event::AddressGossiperIncoming(incoming) => reactor::wrap_effects(
                Event::AddressGossiper,
                self.address_gossiper
                    .handle_event(effect_builder, rng, incoming.into()),
            ),
        }
    }

    fn maybe_exit(&self) -> Option<crate::reactor::ReactorExit> {
        unimplemented!()
    }
}

impl NetworkedReactor for TestReactor {
    fn node_id(&self) -> NodeId {
        self.net.node_id()
    }
}

impl Finalize for TestReactor {
    fn finalize(self) -> futures::future::BoxFuture<'static, ()> {
        self.net.finalize()
    }
}

/// Checks whether or not a given network with potentially blocked nodes is completely connected.
fn network_is_complete(
    blocklist: &HashSet<NodeId>,
    nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<TestReactor>>>,
) -> bool {
    // Collect expected nodes.
    let expected: HashSet<_> = nodes
        .keys()
        .filter(|&node_id| !blocklist.contains(node_id))
        .copied()
        .collect();

    for (node_id, node) in nodes {
        let net = &node.reactor().inner().net;
        // TODO: Ensure the connections are symmetrical.
        let peers: HashSet<_> = net.peers().into_iter().map(|(k, _)| k).collect();

        let mut missing = expected.difference(&peers);

        if let Some(first_missing) = missing.next() {
            // We only allow loopbacks to be missing.
            if first_missing != node_id {
                return false;
            }
        }

        if missing.next().is_some() {
            // We have at least two missing, which cannot be.
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
#[cfg(not(target_os = "macos"))]
#[tokio::test]
async fn bind_to_real_network_interface() {
    init_logging();

    let mut rng = crate::new_rng();

    let iface = pnet::datalink::interfaces()
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

        // Now the network should have an appropriate number of peers.

        // This test will run multiple times, so ensure we cleanup all ports.
        net.finalize().await;
    }
}

/// Check that a network of varying sizes will connect all nodes properly.
#[tokio::test]
async fn ensure_peers_metric_is_correct() {
    init_logging();

    let mut rng = crate::new_rng();

    // Larger networks can potentially become more unreliable, so we try with small sizes only.
    for &number_of_nodes in &[2u16, 3, 5] {
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

        net.settle_on(
            &mut rng,
            |nodes: &Nodes<TestReactor>| {
                nodes.values().all(|runner| {
                    runner.reactor().inner().net.net_metrics.peers.get()
                        == number_of_nodes as i64 - 1
                })
            },
            timeout,
        )
        .await;

        net.finalize().await;
    }
}
