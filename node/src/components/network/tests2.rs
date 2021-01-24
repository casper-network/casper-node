use std::{
    collections::HashMap,
    env,
    fmt::{Debug, Display},
    time::Duration,
};

use rand::{distributions::Standard, Rng};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{
    effect::EffectExt, reactor::Runner, testing, testing::TestRng, types::NodeId, Chainspec,
};
use casper_node_macros::reactor;
use testing::{init_logging, network::NetworkedReactor, ConditionCheckReactor};

use super::ENABLE_LIBP2P_ENV_VAR;

// Reactor for load testing, whose networking component just sends dummy payloads around.
reactor!(LoadTestingReactor {
  type Config = TestReactorConfig;

  components: {
      net = has_effects Network::<LoadTestingReactorEvent, DummyPayload>(
        event_queue, cfg.network_config, &cfg.chainspec, false
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
    chainspec: Chainspec,
    /// Network configuration used in testing.
    network_config: crate::components::network::Config,
}

/// A dummy payload.
#[derive(Clone, Eq, Deserialize, PartialEq, Serialize)]
pub struct DummyPayload(Vec<u8>);

impl DummyPayload {
    fn random_with_size(rng: &mut TestRng, sz: usize) -> Self {
        DummyPayload(rng.sample_iter(Standard).take(sz).collect())
    }
}

impl Debug for DummyPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "payload ({} bytes: {:?}...)",
            self.0.len(),
            &self.0[0..self.0.len().min(10)]
        )
    }
}

impl Display for DummyPayload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(self, f)
    }
}

#[tokio::test]
async fn send_large_message_across_network() {
    init_logging();

    if env::var(ENABLE_LIBP2P_ENV_VAR).is_err() {
        eprintln!("{} not set, skipping test", ENABLE_LIBP2P_ENV_VAR);
        return;
    }

    let node_count: usize = 20;
    let timeout = Duration::from_secs(5);
    let large_size: usize = 512;

    let mut rng = crate::new_rng();

    // Port for first node, other will connect to it.
    let first_node_port = testing::unused_port_on_localhost() + 1;

    let mut net = testing::network::Network::<LoadTestingReactor>::new();
    let chainspec = Chainspec::random(&mut rng);

    // Create the root node.
    let cfg = TestReactorConfig {
        chainspec: chainspec.clone(),
        network_config: crate::components::network::Config::default_local_net_first_node(
            first_node_port,
        ),
    };

    net.add_node_with_config(cfg, &mut rng).await.unwrap();

    // Create `node_count-1` additional node instances.
    for _ in 1..node_count {
        let cfg = TestReactorConfig {
            chainspec: chainspec.clone(),
            network_config: crate::components::network::Config::default_local_net(first_node_port),
        };

        net.add_node_with_config(cfg, &mut rng).await.unwrap();
    }

    info!("Network setup, waiting for discovery to complete");
    net.settle_on(&mut rng, network_online, timeout).await;

    // At this point each node has at least one other peer. Assuming no split, we can now start
    // gossiping a large payloads. We gossip one on each node.
    let node_ids: Vec<_> = net.nodes().keys().cloned().collect();
    for sender in &node_ids {
        let dummy_payload = DummyPayload::random_with_size(&mut rng, large_size);

        // Calling `broadcast_message` actually triggers libp2p gossping.
        net.process_injected_effect_on(sender, |effect_builder| {
            effect_builder
                .broadcast_message(dummy_payload.clone())
                .ignore()
        })
        .await;

        info!(payload = %dummy_payload,
              "Started gossip of payload, waiting for all nodes to receive it");
        net.settle_on(&mut rng, everyone_received(&dummy_payload), timeout)
            .await;
    }
}

/// Checks if all nodes are connected to at least one other node.
pub fn network_online(
    nodes: &HashMap<NodeId, Runner<ConditionCheckReactor<LoadTestingReactor>>>,
) -> bool {
    assert!(
        nodes.len() >= 3,
        "cannot check for an online network with less than 3 nodes"
    );

    // If we have no isolated nodes, the network is online. Note that we do not check for a split
    // here.
    !nodes
        .values()
        .any(|runner| dbg!(runner.reactor().inner().net.known_addresses().len()) == 3)
}

/// Checks whether or not every node on the network received the provied payload.
pub fn everyone_received<'a>(
    payload: &'a DummyPayload,
) -> impl Fn(&HashMap<NodeId, Runner<ConditionCheckReactor<LoadTestingReactor>>>) -> bool + 'a {
    move |nodes| {
        nodes.values().all(|runner| {
            runner
                .reactor()
                .inner()
                .collector
                .payloads
                .contains(payload)
        })
    }
}
