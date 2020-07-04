//! Tests for the `small_network` component.
//!
//! Calling these "unit tests" would be a bit of a misnomer, since they deal mostly with multiple
//! instances of `small_net` arranged in a network.

use std::{
    collections::{hash_map::Entry, HashMap, HashSet},
    fmt::{self, Debug, Display, Formatter},
    time::Duration,
};

use derive_more::From;
use futures::{future::join_all, Future};
use serde::{Deserialize, Serialize};
use small_network::NodeId;

use crate::{
    components::Component,
    effect::{announcements::NetworkAnnouncement, Effect, EffectBuilder, Multiple},
    logging,
    reactor::{self, EventQueueHandle, Reactor},
    small_network::{self, SmallNetwork},
};
use pnet::datalink;
use tokio::time::{timeout, Timeout};
use tracing::{debug, field, info, Span};

/// Time interval for which to poll an observed testing network when no events have occurred.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

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

    fn dispatch_event(
        &mut self,
        effect_builder: EffectBuilder<Self::Event>,
        event: Self::Event,
    ) -> Multiple<Effect<Self::Event>> {
        let mut rng = rand::thread_rng(); // FIXME: Pass this in from the outside?

        match event {
            Event::SmallNet(ev) => reactor::wrap_effects(
                Event::SmallNet,
                self.net.handle_event(effect_builder, &mut rng, ev),
            ),
        }
    }

    fn new(
        cfg: Self::Config,
        event_queue: EventQueueHandle<Self::Event>,
        span: &Span,
    ) -> anyhow::Result<(Self, Multiple<Effect<Self::Event>>)> {
        let (net, effects) = SmallNetwork::new(event_queue, cfg)?;

        let node_id = net.node_id();
        span.record("id", &field::display(node_id));

        Ok((
            TestReactor { net },
            reactor::wrap_effects(Event::SmallNet, effects),
        ))
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

/// A network of multiple `small_network` test reactors.
///
/// Nodes themselves are not run in the background, rather manual cranking is required through
/// `crank_all`. As an alternative, the `settle` and `settle_all` functions can be used to continue
/// cranking until a condition has been reached.
#[derive(Debug)]
struct Network {
    node_count: usize,
    nodes: HashMap<NodeId, reactor::Runner<TestReactor>>,
}

impl Network {
    /// Creates a new network.
    fn new() -> Self {
        Network {
            node_count: 0,
            nodes: HashMap::new(),
        }
    }

    /// Creates a new networking node on the network using the default root node port.
    async fn add_node(&mut self) -> anyhow::Result<(NodeId, &mut reactor::Runner<TestReactor>)> {
        self.add_node_with_config(small_network::Config::default_on_port(TEST_ROOT_NODE_PORT))
            .await
    }

    /// Creates a new networking node on the network.
    async fn add_node_with_config(
        &mut self,
        cfg: small_network::Config,
    ) -> anyhow::Result<(NodeId, &mut reactor::Runner<TestReactor>)> {
        let runner: reactor::Runner<TestReactor> = reactor::Runner::new(cfg).await?;
        self.node_count += 1;

        let node_id = runner.reactor().net.node_id();

        let node_ref = match self.nodes.entry(node_id) {
            Entry::Occupied(_) => {
                // This happens in the event of the extremely unlikely hash collision, or if the
                // node ID was set manually.
                anyhow::bail!("trying to insert a duplicate node {}", node_id)
            }
            Entry::Vacant(entry) => entry.insert(runner),
        };

        Ok((node_id, node_ref))
    }

    /// Crank all runners once, returning the number of events processed.
    async fn crank_all(&mut self) -> usize {
        join_all(self.nodes.values_mut().map(reactor::Runner::try_crank))
            .await
            .into_iter()
            .filter(|opt| opt.is_some())
            .count()
    }

    /// Process events on all nodes until all event queues are empty.
    ///
    /// Exits if `at_least` time has passed twice between events that have been processed.
    async fn settle(&mut self, at_least: Duration) {
        let mut no_events = false;
        loop {
            if self.crank_all().await == 0 {
                // Stop once we have no pending events and haven't had any for `at_least` duration.
                if no_events {
                    debug!(?at_least, "network has settled after");
                    break;
                } else {
                    no_events = true;
                    tokio::time::delay_for(at_least).await;
                }
            } else {
                no_events = false;
            }
        }
    }

    /// Runs the main loop of every reactor until a condition is true.
    async fn settle_on<F>(&mut self, f: F)
    where
        F: Fn(&HashMap<NodeId, reactor::Runner<TestReactor>>) -> bool,
    {
        loop {
            // Check condition.
            if f(&self.nodes) {
                debug!("network settled");
                break;
            }

            if self.crank_all().await == 0 {
                // No events processed, wait for a bit to avoid 100% cpu usage.
                tokio::time::delay_for(POLL_INTERVAL).await;
            }
        }
    }

    // Returns the internal map of nodes.
    fn nodes(&self) -> &HashMap<NodeId, reactor::Runner<TestReactor>> {
        &self.nodes
    }

    /// Shuts down the network.
    ///
    /// Shuts down the network, allowing all connections to terminate. This is the same as dropping
    /// every node and waiting until every networking instance has completely shut down.
    ///
    /// Usually dropping is enough, but when attempting to reusing listening ports immediately, this
    /// gets the job done.
    async fn shutdown(self) {
        // Shutdown the sender of every reactor node to ensure the port is open again.
        for (_, node) in self.nodes.into_iter() {
            node.into_inner().net.shutdown_server().await;
        }

        debug!("shut down network");
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
fn network_is_complete(nodes: &HashMap<NodeId, reactor::Runner<TestReactor>>) -> bool {
    // We need at least one node.
    if nodes.is_empty() {
        return false;
    }

    let expected: HashSet<_> = nodes
        .iter()
        .map(|(_, runner)| runner.reactor().net.node_id())
        .collect();

    nodes
        .iter()
        .map(|(_, runner)| {
            let net = &runner.reactor().net;
            let mut actual = net.connected_nodes();

            // All nodes should be connected to every other node, except itself, so we add it to the
            // set of nodes and pretend we have a loopback connection.
            actual.insert(net.node_id());

            actual
        })
        .all(|actual| actual == expected)
}

/// Helper trait to annotate timeouts more naturally.
trait Within<T> {
    /// Sets a timeout on a future.
    ///
    /// If the timeout occurs, the annotated future will be **cancelled**. Use with caution.
    fn within(self, duration: Duration) -> Timeout<T>;
}

impl<T> Within<T> for T
where
    T: Future,
{
    fn within(self, duration: Duration) -> Timeout<T> {
        timeout(duration, self)
    }
}

/// Run a two-node network five times.
///
/// Ensures that network cleanup and basic networking works.
#[tokio::test]
async fn run_two_node_network_five_times() {
    init_logging();

    for i in 0..5 {
        info!("two-network test round {}", i);

        let mut net = Network::new();

        let start = std::time::Instant::now();
        net.add_node().await.unwrap();
        net.add_node().await.unwrap();
        let end = std::time::Instant::now();

        debug!(
            total_time_ms = (end - start).as_millis() as u64,
            "finished setting up networking nodes"
        );

        net.settle_on(network_is_complete)
            .within(Duration::from_millis(1000))
            .await
            .expect("network did not fully connect in time");

        net.settle(Duration::from_millis(25))
            .within(Duration::from_millis(2000))
            .await
            .expect("network did not stay settled");

        assert!(
            network_is_complete(net.nodes()),
            "network did not stay connected"
        );

        net.shutdown().await;
    }
}

/// Sanity check that we can bind to a real network.
///
/// Very unlikely to ever fail on a real machine.
#[tokio::test]
async fn bind_to_real_network_interface() {
    init_logging();

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
        private_key: None,
    };

    let mut net = Network::new();
    net.add_node_with_config(local_net_config).await.unwrap();

    net.settle(Duration::from_millis(250)).await;
}
