#![allow(unreachable_code)] // TODO: Figure out why this warning triggers.

use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
    env, fmt,
    fmt::{Debug, Display, Formatter},
    str::FromStr,
    sync::Arc,
    thread,
    time::Duration,
};

use libp2p::kad::kbucket::K_VALUE;
use rand::{distributions::Standard, Rng};
use serde::{Deserialize, Serialize};
use tracing::info;

use casper_node_macros::reactor;

use super::ENABLE_LIBP2P_NET_ENV_VAR;
use crate::{
    components::{
        collector::Collectable,
        network::{Config as NetworkComponentConfig, NetworkIdentity},
    },
    effect::EffectExt,
    reactor::Runner,
    testing::{
        self,
        network::{Network as TestingNetwork, NetworkedReactor},
        ConditionCheckReactor, TestRng,
    },
    types::{Chainspec, NodeId},
};

// Reactor for load testing, whose networking component just sends dummy payloads around.
reactor!(LoadTestingReactor {
  type Config = TestReactorConfig;

  components: {
      net = has_effects Network::<LoadTestingReactorEvent, DummyPayload>(
        event_queue, cfg.network_config, registry, NetworkIdentity::new(), &cfg.chainspec, false
      );
      collector = infallible Collector::<DummyPayload>();
  }

  events: {
      net = Event<DummyPayload>;
      collector = Event<DummyPayload>;
  }

  requests: {
      NetworkRequest<NodeId, DummyPayload> -> net;
  }

  announcements: {
      NetworkAnnouncement<NodeId, DummyPayload> -> [collector];
  }
});

impl NetworkedReactor for LoadTestingReactor {
    type NodeId = NodeId;

    fn node_id(&self) -> Self::NodeId {
        self.net.node_id()
    }
}

/// Configuration for the test reactor.
#[derive(Debug)]
pub struct TestReactorConfig {
    /// The fixed chainspec used in testing.
    chainspec: Arc<Chainspec>,
    /// Network configuration used in testing.
    network_config: NetworkComponentConfig,
}

/// A dummy payload.
#[derive(Clone, Eq, Deserialize, Hash, PartialEq, Serialize)]
pub struct DummyPayload(Vec<u8>);

const DUMMY_PAYLOAD_ID_LEN: usize = 16;

/// ID of a dummy payload.
type DummyPayloadId = [u8; DUMMY_PAYLOAD_ID_LEN];

impl DummyPayload {
    /// Creates a new randomly generated payload.
    ///
    /// # Panics
    ///
    /// Panics if `sz` is less than `DUMMY_PAYLOAD_ID_LEN` bytes.
    fn random_with_size(rng: &mut TestRng, sz: usize) -> Self {
        assert!(
            sz >= DUMMY_PAYLOAD_ID_LEN,
            "payload must be large enough to derive ID"
        );

        DummyPayload(rng.sample_iter(Standard).take(sz).collect())
    }

    /// Returns the ID of the payload.
    fn id(&self) -> DummyPayloadId {
        TryFrom::try_from(&self.0[..DUMMY_PAYLOAD_ID_LEN])
            .expect("could not get ID data from buffer slice")
    }
}

impl Collectable for DummyPayload {
    type CollectedType = DummyPayloadId;

    fn into_collectable(self) -> Self::CollectedType {
        self.id()
    }
}

impl Debug for DummyPayload {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "payload ({} bytes: {:?}...)",
            self.0.len(),
            &self.0[0..self.0.len().min(10)]
        )
    }
}

impl Display for DummyPayload {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, f)
    }
}

/// Reads an envvar from the environment and, if present, parses it.
///
/// Only absent envvars are returned as `None`.
///
/// # Panics
///
/// Panics on any parse error.
fn read_env<T: FromStr>(name: &str) -> Option<T>
where
    <T as FromStr>::Err: Debug,
{
    match env::var(name) {
        Ok(raw) => Some(
            raw.parse()
                .unwrap_or_else(|_| panic!("cannot parse envvar `{}`", name)),
        ),
        Err(env::VarError::NotPresent) => None,
        Err(err) => {
            panic!(err)
        }
    }
}

// TODO - investigate why this fails on CI.
// DONE - probably because we are not running with --release!
#[ignore]
#[tokio::test]
async fn send_large_message_across_network() {
    testing::init_logging();

    if env::var(ENABLE_LIBP2P_NET_ENV_VAR).is_err() {
        eprintln!("{} set, skipping test", ENABLE_LIBP2P_NET_ENV_VAR);
        return;
    }

    // This can, on a decent machine, be set to 30, 50, maybe even 100 nodes. The default is set to
    // 5 to avoid overloading CI.
    let node_count: usize = read_env("TEST_NODE_COUNT").unwrap_or(5);

    // Fully connecting a 20 node network takes ~ 3 seconds. This should be ample time for gossip
    // and connecting.
    let timeout = Duration::from_secs(60);
    let payload_size: usize = read_env("TEST_PAYLOAD_SIZE").unwrap_or(1024 * 1024 * 4);
    let payload_count: usize = read_env("TEST_PAYLOAD_COUNT").unwrap_or(1);

    let mut rng = crate::new_rng();

    // Port for first node, other will connect to it.
    let first_node_port = testing::unused_port_on_localhost() + 1;

    let mut net = TestingNetwork::<LoadTestingReactor>::new();
    let chainspec = Arc::new(Chainspec::random(&mut rng));

    // Create the root node.
    let cfg = TestReactorConfig {
        chainspec: Arc::clone(&chainspec),
        network_config: NetworkComponentConfig::default_local_net_first_node(first_node_port),
    };

    net.add_node_with_config(cfg, &mut rng).await.unwrap();

    // Hack to get network component to connect. This gives the libp2p thread (which is independent
    // of cranking) a little time to bind to the socket.
    thread::sleep(Duration::from_secs(2));

    // Create `node_count-1` additional node instances.
    for _ in 1..node_count {
        let cfg = TestReactorConfig {
            chainspec: Arc::clone(&chainspec),
            network_config: NetworkComponentConfig::default_local_net(first_node_port),
        };

        net.add_node_with_config(cfg, &mut rng).await.unwrap();
    }

    info!("Network setup, waiting for discovery to complete");
    net.settle_on(&mut rng, network_online, timeout).await;
    info!("Discovery complete");

    // At this point each node has at least one other peer. Assuming no split, we can now start
    // gossiping large payloads. We gossip one on each node.
    let node_ids: Vec<_> = net.nodes().keys().cloned().collect();
    for (index, sender) in node_ids.iter().enumerate() {
        // Clear all collectors at the beginning of a round.
        net.reactors_mut()
            .for_each(|reactor| reactor.collector.payloads.clear());

        let mut dummy_payloads = HashSet::new();
        let mut dummy_payload_ids = HashSet::new();

        // Prepare a set of dummy payloads.
        for _ in 0..payload_count {
            let payload = DummyPayload::random_with_size(&mut rng, payload_size);
            dummy_payload_ids.insert(payload.id());
            dummy_payloads.insert(payload);
        }

        for dummy_payload in &dummy_payloads {
            // Calling `broadcast_message` actually triggers libp2p gossping.
            net.process_injected_effect_on(sender, |effect_builder| {
                effect_builder
                    .broadcast_message(dummy_payload.clone())
                    .ignore()
            })
            .await;
        }

        info!(?sender, num_payloads = %dummy_payloads.len(), round=index, total_rounds=node_ids.len(),
              "Started broadcast/gossip of payloads, waiting for all nodes to receive it");
        net.settle_on(
            &mut rng,
            others_received(dummy_payload_ids, sender.clone()),
            timeout,
        )
        .await;
        info!(?sender, "Completed gossip test for sender")
    }
}

/// Checks if all nodes are connected to at least one other node.
fn network_online(
    nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<LoadTestingReactor>>>,
) -> bool {
    assert!(
        nodes.len() >= 2,
        "cannot check for an online network with less than 3 nodes"
    );

    let k_value = usize::from(K_VALUE);

    // Sanity check of K_VALUE.
    assert!(
        k_value >= 7,
        "K_VALUE is really small, expected it to be at least 7"
    );

    // The target of known nodes to go for. This has a hard bound of `K_VALUE`, since if all nodes
    // end up in the same bucket, we will start evicting them. In general, we go for K_VALUE/2 for
    // reasonable interconnection, or the network size - 1, which is another bound.
    let known_nodes_target = (k_value / 2).min(nodes.len() - 1);

    // Checks if all nodes have reached the known nodes target.
    nodes
        .values()
        .all(|runner| runner.reactor().inner().net.seen_peers().len() >= known_nodes_target)
}

/// Checks whether or not every node except `sender` on the network received the given payload.
fn others_received(
    payloads: HashSet<DummyPayloadId>,
    sender: NodeId,
) -> impl Fn(&HashMap<NodeId, Runner<ConditionCheckReactor<LoadTestingReactor>>>) -> bool {
    move |nodes| {
        nodes
            .values()
            // We're only interested in the inner reactor.
            .map(|runner| runner.reactor().inner())
            // Skip the sender.
            .filter(|reactor| reactor.node_id() != sender)
            // Ensure others all have received the payload.
            // Note: `std` HashSet short circuits on length, so this should be fine.
            .all(|reactor| reactor.collector.payloads == payloads)
    }
}
