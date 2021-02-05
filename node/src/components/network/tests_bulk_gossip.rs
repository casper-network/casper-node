use std::{
    collections::{HashMap, HashSet},
    convert::TryFrom,
    env, fmt,
    fmt::{Debug, Display, Formatter},
    fs::File,
    path::PathBuf,
    str::FromStr,
    time::Duration,
};

use hex_fmt::HexFmt;
use libp2p::kad::kbucket::K_VALUE;
use rand::{distributions::Standard, Rng};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use tracing::info;

use crate::{
    components::{
        collector::Collectable,
        network::{Config as NetworkComponentConfig, NetworkIdentity},
    },
    effect::EffectExt,
    testing,
    testing::{network::Network as TestingNetwork, TestRng},
    types::{Deploy, NodeId},
    Chainspec,
};
use casper_node_macros::reactor;
use testing::{
    init_logging,
    network::{NetworkedReactor, Nodes},
};

use super::ENABLE_SMALL_NET_ENV_VAR;

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
      GossipAnnouncement<Deploy> -> [];
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
                .expect(&format!("cannot parse envvar `{}`", name)),
        ),
        Err(env::VarError::NotPresent) => None,
        Err(err) => {
            panic!(err)
        }
    }
}

/// Load a configuration for a test from an environment variable.
///
/// `var_name` dictates the name of the environment variable the configuration is loaded from. The
/// variable must contain the path to a JSON file from which the data is deserialized.
///
/// If `var_name` is not set, returns the `Default::default` value.
///
/// # Panics
///
/// If the envvar is pointing to an invalid or nonexistant filename, or the input file is malformed,
/// this function will panic.
fn load_test_config<T>(var_name: &str) -> T
where
    T: Debug + Default + DeserializeOwned + Serialize,
{
    let cfg = if let Some(cfg_path) = read_env::<PathBuf>(var_name) {
        let cfg = serde_json::from_reader(
            File::open(cfg_path).expect("failure opening test configuration"),
        )
        .expect("failure parsing test configuration");
        println!("Loaded test configuration from {}.", var_name);
        cfg
    } else {
        let cfg = Default::default();
        println!("Using default configuration.");
        cfg
    };

    // Dump the test configuration.
    println!(
        "{}",
        serde_json::to_string_pretty(&cfg)
            .expect("could not serialize test configuration for output")
    );

    cfg
}

/// Configuration for a message sending test.
#[derive(Debug, Deserialize, Serialize)]
struct MessageSendTestConfig {
    /// Number of nodes to include in test.
    node_count: usize,
    /// Size of the payload to send.
    payload_size: usize,
    /// How often to send the payload.
    payload_count: usize,
    /// How long to wait for a round to reach saturation before quitting.
    full_gossip_round_timeout: u64,
    /// How long to wait for the network to fully discover itself.
    discovery_timeout: u64,
    /// Number of seconds to wait after binding first node.
    bind_delay: u64,
}

impl Default for MessageSendTestConfig {
    fn default() -> Self {
        MessageSendTestConfig {
            node_count: 5,
            payload_size: 1024 * 1024 * 4,
            payload_count: 1,
            full_gossip_round_timeout: 20,
            discovery_timeout: 30,
            bind_delay: 2,
        }
    }
}

#[tokio::test]
async fn message_sending_test() {
    init_logging();

    let test_cfg: MessageSendTestConfig = load_test_config("TESTCFG_SEND_MESSAGE");

    if env::var(ENABLE_SMALL_NET_ENV_VAR).is_ok() {
        eprintln!("{} set, skipping test", ENABLE_SMALL_NET_ENV_VAR);
        return;
    }

    let mut rng = crate::new_rng();

    // Port for first node, other will connect to it.
    let first_node_port = testing::unused_port_on_localhost() + 1;

    let mut net = TestingNetwork::<LoadTestingReactor>::new();
    let chainspec = Chainspec::random(&mut rng);

    // Create the root node.
    let cfg = TestReactorConfig {
        chainspec: chainspec.clone(),
        network_config: NetworkComponentConfig::default_local_net_first_node(first_node_port),
    };

    net.add_node_with_config(cfg, &mut rng).await.unwrap();

    // Hack to get network component to connect. This gives the libp2p thread (which is independent
    // of cranking) a little time to bind to the socket.
    std::thread::sleep(std::time::Duration::from_secs(test_cfg.bind_delay));

    // Create `node_count-1` additional node instances.
    for _ in 1..test_cfg.node_count {
        let cfg = TestReactorConfig {
            chainspec: chainspec.clone(),
            network_config: NetworkComponentConfig::default_local_net(first_node_port),
        };

        net.add_node_with_config(cfg, &mut rng).await.unwrap();
    }

    info!("Network setup, waiting for discovery to complete");
    // Fully connecting a 20 node network takes ~ 3 seconds. This should be ample time.
    net.settle_on(
        &mut rng,
        network_online,
        Duration::from_secs(test_cfg.discovery_timeout),
    )
    .await;
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
        for _ in 0..test_cfg.payload_count {
            let payload = DummyPayload::random_with_size(&mut rng, test_cfg.payload_size);
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
        net.try_settle_on(
            &mut rng,
            others_received(dummy_payload_ids, sender.clone()),
            Duration::from_secs(test_cfg.full_gossip_round_timeout),
        )
        .await
        .unwrap_or_else(|elapsed| {
            let dump_filename = "test_run_network_state.json";

            let state: HashMap<_, _> = net
                .reactors()
                .map(|reactor| {
                    (
                        reactor.node_id().to_string(),
                        reactor
                            .collector
                            .payloads
                            .iter()
                            .map(|payload_id| HexFmt(payload_id).to_string())
                            .collect::<Vec<_>>(),
                    )
                })
                .collect();

            // TODO: Allow for general saving of test outcome state using trait type. Use to collect
            // diagnostics in general.
            let mut dump_file = File::create(dump_filename).expect("cannot open dump filename");
            serde_json::to_writer_pretty(&mut dump_file, &state)
                .expect("failed to dump node state");

            drop(dump_file);

            panic!(
                "failed after {}, network gossip state has been dumped to {}",
                elapsed, dump_filename
            )
        });
        info!(?sender, "Completed gossip test for sender")
    }
}

/// Checks if all nodes are connected to at least one other node.
fn network_online(nodes: &Nodes<LoadTestingReactor>) -> bool {
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
) -> impl Fn(&Nodes<LoadTestingReactor>) -> bool {
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
