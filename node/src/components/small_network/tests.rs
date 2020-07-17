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
use serde::{Deserialize, Serialize};
use small_network::NodeId;

use crate::{
    components::Component,
    effect::{announcements::NetworkAnnouncement, EffectBuilder, Effects},
    logging,
    reactor::{self, EventQueueHandle, Reactor, Runner},
    small_network::{self, SmallNetwork},
    testing::{
        network::{Network, NetworkedReactor},
        ConditionCheckReactor,
    },
};
use pnet::datalink;
use prometheus::Registry;
use rand::{rngs::OsRng, Rng};
use reactor::{wrap_effects, Finalize};
use tracing::{debug, info};

/// The networking port used by the tests for the root node.
const TEST_ROOT_NODE_PORT: u16 = 11223;

/// Test-reactor event.
#[derive(Debug, From)]
enum Event {
    #[from]
    SmallNet(small_network::Event<Message>),
}

/// Example message.
///
/// All messages are empty, currently the tests are not checking the payload.
#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
struct Message;

/// Test reactor.
///
/// Runs a single small network.
#[derive(Debug)]
struct TestReactor {
    net: SmallNetwork<Event, Message>,
}

impl From<NetworkAnnouncement<NodeId, Message>> for Event {
    fn from(_: NetworkAnnouncement<NodeId, Message>) -> Self {
        // This will be called if a message is sent, thus it is currently not implemented.
        todo!()
    }
}

impl Reactor for TestReactor {
    type Event = Event;
    type Config = small_network::Config;
    type Error = anyhow::Error;

    fn dispatch_event<R: Rng + ?Sized>(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        rng: &mut R,
        event: Self::Event,
    ) -> Effects<Self::Event> {
        match event {
            Event::SmallNet(ev) => wrap_effects(
                Event::SmallNet,
                self.net.handle_event(effect_builder, rng, ev),
            ),
        }
    }

    fn new<R: Rng + ?Sized>(
        cfg: Self::Config,
        _registry: &Registry,
        event_queue: EventQueueHandle<Self::Event>,
        _rng: &mut R,
    ) -> anyhow::Result<(Self, Effects<Self::Event>)> {
        let (net, effects) = SmallNetwork::new(event_queue, cfg)?;

        Ok((TestReactor { net }, wrap_effects(Event::SmallNet, effects)))
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

impl Display for Event {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Event::SmallNet(ev) => Display::fmt(ev, f),
        }
    }
}

impl Display for Message {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

/// Sets up logging for testing.
///
/// Returns a guard that when dropped out of scope, clears the logger again.
fn init_logging() {
    // TODO: Write logs to file by default for each test.
    logging::init()
        // Ignore the return value, setting the global subscriber will fail if `init_logging` has
        // been called before, which we don't care about.
        .ok();
}

/// Checks whether or not a given network is completely connected.
fn network_is_complete(
    nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<TestReactor>>>,
) -> bool {
    // We need at least one node.
    if nodes.is_empty() {
        return false;
    }

    let expected: HashSet<_> = nodes
        .iter()
        .map(|(_, runner)| runner.reactor().inner().net.node_id())
        .collect();

    nodes
        .iter()
        .map(|(_, runner)| {
            let net = &runner.reactor().inner().net;
            let mut actual = net.connected_nodes();

            // All nodes should be connected to every other node, except itself, so we add it to the
            // set of nodes and pretend we have a loopback connection.
            actual.insert(net.node_id());

            actual
        })
        .all(|actual| actual == expected)
}

fn gen_config(bind_port: u16) -> small_network::Config {
    // Bind everything to localhost.
    let bind_interface = "127.0.0.1".parse().unwrap();

    small_network::Config {
        bind_interface,
        bind_port,
        root_addr: (bind_interface, TEST_ROOT_NODE_PORT).into(),
        // Fast retry, moderate amount of times. This is 10 seconds max (100 x 100 ms)
        max_outgoing_retries: Some(100),
        outgoing_retry_delay_millis: 100,
        // Auto-generate cert and key.
        cert: None,
        secret_key: None,
    }
}

/// Run a two-node network five times.
///
/// Ensures that network cleanup and basic networking works.
#[tokio::test]
async fn run_two_node_network_five_times() {
    let mut rng = OsRng;

    init_logging();

    for i in 0..5 {
        info!("two-network test round {}", i);

        let mut net = Network::new();

        let start = Instant::now();
        net.add_node_with_config(gen_config(TEST_ROOT_NODE_PORT), &mut rng)
            .await
            .unwrap();
        net.add_node_with_config(gen_config(TEST_ROOT_NODE_PORT + 1), &mut rng)
            .await
            .unwrap();
        let end = Instant::now();

        debug!(
            total_time_ms = (end - start).as_millis() as u64,
            "finished setting up networking nodes"
        );

        let timeout = Duration::from_secs(1);
        net.settle_on(&mut rng, network_is_complete, timeout).await;

        let quiet_for = Duration::from_millis(25);
        let timeout = Duration::from_secs(2);
        net.settle(&mut rng, quiet_for, timeout).await;

        assert!(
            network_is_complete(net.nodes()),
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
    init_logging();

    let mut rng = OsRng;

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
    let port = TEST_ROOT_NODE_PORT;

    let local_net_config = small_network::Config {
        bind_interface: local_addr,
        bind_port: port,
        root_addr: (local_addr, port).into(),
        max_outgoing_retries: Some(360),
        outgoing_retry_delay_millis: 10000,
        cert: None,
        secret_key: None,
    };

    let mut net = Network::<TestReactor>::new();
    net.add_node_with_config(local_net_config, &mut rng)
        .await
        .unwrap();

    let quiet_for = Duration::from_millis(250);
    let timeout = Duration::from_secs(2);
    net.settle(&mut rng, quiet_for, timeout).await;
}
