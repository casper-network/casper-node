use std::{
    collections::HashSet,
    fmt::{self, Debug, Display, Formatter},
    io,
    time::Duration,
};

use derive_more::From;
use futures::{future::join_all, Future};
use serde::{Deserialize, Serialize};
use small_network::NodeId;
use tracing_subscriber::{self, EnvFilter};

use crate::{
    components::Component,
    effect::{announcements::NetworkAnnouncement, Effect, EffectBuilder, Multiple},
    reactor::{self, EventQueueHandle, Reactor},
    small_network::{self, SmallNetwork},
};
use tokio::time::{timeout, Timeout};
use tracing::{debug, info};

/// Time interval for which to poll an observed testing network when no events have occured.
const POLL_INTERVAL: Duration = Duration::from_millis(50);

/// Amount of time to wait after shutting down all nodes to give the OS networking stack time to
/// catch up.
const NET_COOLDOWN: Duration = Duration::from_millis(100);

#[derive(Debug, From)]
enum Event {
    #[from]
    SmallNet(small_network::Event<Message>),
}

#[derive(Copy, Clone, Debug, Deserialize, Serialize)]
struct Message;

#[derive(Debug)]
struct TestReactor {
    net: SmallNetwork<Event, Message>,
}

impl From<NetworkAnnouncement<NodeId, Message>> for Event {
    fn from(_: NetworkAnnouncement<NodeId, Message>) -> Self {
        todo!()
    }
}

impl Reactor for TestReactor {
    type Event = Event;
    type Config = small_network::Config;

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
    ) -> reactor::Result<(Self, Multiple<Effect<Self::Event>>)> {
        let (net, effects) = SmallNetwork::new(event_queue, cfg)?;

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

#[derive(Debug)]
struct Network {
    nodes: Vec<reactor::Runner<TestReactor>>,
}

impl Network {
    /// Creates a new network.
    fn new() -> Self {
        Network { nodes: Vec::new() }
    }

    /// Creates a new networking node on the network.
    async fn add_node(&mut self) -> anyhow::Result<&mut reactor::Runner<TestReactor>> {
        let runner = reactor::Runner::new(small_network::Config::default_on_port(11223)).await?;
        self.nodes.push(runner);

        Ok(self
            .nodes
            .last_mut()
            .expect("vector cannot be empty after insertion"))
    }

    /// Crank all runners once, returning the number of events processed.
    async fn crank_all(&mut self) -> usize {
        join_all(self.nodes.iter_mut().map(|runner| runner.try_crank()))
            .await
            .into_iter()
            .filter(|opt| opt.is_some())
            .count()
    }

    /// Process events on all nodes until all queues are empty.
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
        F: Fn(&[reactor::Runner<TestReactor>]) -> bool,
    {
        loop {
            // Check condition.
            if f(&self.nodes) {
                debug!("network settled");
                break;
            }

            if self.crank_all().await == 0 {
                // No events processed, wait for a bit
                tokio::time::delay_for(POLL_INTERVAL).await;
            }
        }
    }

    // Returns the internal list of nodes.
    fn nodes(&self) -> &[reactor::Runner<TestReactor>] {
        &self.nodes
    }

    /// Shut down the network.
    ///
    /// Shuts down the network, allowing all connections to terminate. This is the same as dropping
    /// every node and waiting until every networking instance has completely shut down.
    ///
    /// Usually dropping is enough, but when attempting to reusing listening ports immediately, this
    /// gets the job done.
    async fn shutdown(self) {
        drop(self);

        debug!("dropped network, waiting for connections to terminate");
        tokio::time::delay_for(NET_COOLDOWN).await;
        debug!("finished waiting for connections to terminate");
    }
}

/// Setup logging for testing.
fn init_logging() {
    // TODO: Write logs to file by default for each test.
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_writer(io::stderr)
            .with_env_filter(EnvFilter::from_default_env())
            .finish(),
    )
    .expect("initialize test logging");
}

/// Checks whether or not a given network is completely connected.
fn network_is_complete(nodes: &[reactor::Runner<TestReactor>]) -> bool {
    // We need at least one node.
    if nodes.is_empty() {
        return false;
    }

    let expected: HashSet<_> = nodes
        .iter()
        .map(|runner| runner.reactor().net.node_id())
        .collect();

    nodes
        .iter()
        .map(|runner| {
            let net = &runner.reactor().net;
            let mut actual = net.connected_nodes();

            // All nodes should be connected to every other node, except itself, so we add it to the
            // set of nodes and pretend we have a loopback connection.
            actual.insert(net.node_id());

            actual
        })
        .all(|actual| actual == expected)
}

trait Within<T> {
    fn within(self, duration: Duration) -> Timeout<T>;
}

impl<T> Within<T> for T
where
    T: Future,
{
    // type Output = T::Output;

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

#[tokio::test]
async fn run_five_node_network() {
    init_logging();

    let mut net = Network::new();

    net.add_node().await.unwrap();
    net.add_node().await.unwrap();
    net.add_node().await.unwrap();
    net.add_node().await.unwrap();
    net.add_node().await.unwrap();

    timeout(
        Duration::from_millis(10000),
        net.settle_on(network_is_complete),
    )
    .await
    .expect("network did not fully connect in time");

    timeout(
        Duration::from_millis(2000),
        net.settle(Duration::from_millis(250)),
    )
    .await
    .expect("network did not stay settled");

    assert!(
        network_is_complete(net.nodes()),
        "network did not stay connected"
    );
}
