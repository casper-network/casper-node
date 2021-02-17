use std::{
    collections::HashMap,
    convert::TryFrom,
    env, fmt,
    fmt::{Debug, Display, Formatter},
    sync::Arc,
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
        event_queue, cfg.network_config, NetworkIdentity::new(), &cfg.chainspec, false
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
#[derive(Clone, Eq, Deserialize, PartialEq, Serialize)]
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

// TODO - investigate why this fails on CI.
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
    let node_count: usize = 5;

    // Fully connecting a 20 node network takes ~ 3 seconds. This should be ample time for gossip
    // and connecting.
    let timeout = Duration::from_secs(60);
    let large_size: usize = 1024 * 1024 * 4;

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
        let dummy_payload = DummyPayload::random_with_size(&mut rng, large_size);

        // Calling `broadcast_message` actually triggers libp2p gossping.
        net.process_injected_effect_on(sender, |effect_builder| {
            effect_builder
                .broadcast_message(dummy_payload.clone())
                .ignore()
        })
        .await;

        info!(?sender, payload = %dummy_payload, round=index, total=node_ids.len(),
              "Started broadcast/gossip of payload, waiting for all nodes to receive it");
        net.settle_on(
            &mut rng,
            others_received(dummy_payload.id(), sender.clone()),
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
    payload_id: DummyPayloadId,
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
            .all(|reactor| reactor.collector.payloads.contains(&payload_id))
    }
}
