//! This module is home to types/functions related to using libp2p's `Kademlia` and `Identify`
//! behaviors, used for peer discovery.

use libp2p::{
    core::{ProtocolName, PublicKey},
    identify::Identify,
    kad::{
        record::store::{MemoryStore, MemoryStoreConfig},
        Kademlia, KademliaConfig,
    },
    PeerId,
};

use super::{Config, ProtocolId};
use crate::types::Chainspec;

/// The inner portion of the `ProtocolId` for the kademlia behavior.  A standard prefix and suffix
/// will be applied to create the full protocol name.
const KADEMLIA_PROTOCOL_NAME_INNER: &str = "kademlia-peer-discovery";

/// Constructs new libp2p kademlia and identify behaviors suitable for peer-discovery.
pub(super) fn new_behaviors(
    config: &Config,
    chainspec: &Chainspec,
    our_public_key: PublicKey,
) -> (Kademlia<MemoryStore>, Identify) {
    let our_peer_id = PeerId::from(our_public_key.clone());

    // We don't intend to actually store anything in the Kademlia DHT, so configure accordingly.
    let memory_store_config = MemoryStoreConfig {
        max_records: 0,
        max_value_bytes: 0,
        ..Default::default()
    };
    let memory_store = MemoryStore::with_config(our_peer_id.clone(), memory_store_config);

    let protocol_id = ProtocolId::new(chainspec, KADEMLIA_PROTOCOL_NAME_INNER);
    let mut kademlia_config = KademliaConfig::default();
    kademlia_config
        .set_protocol_name(protocol_id.protocol_name().to_vec())
        // Require iterative queries to use disjoint paths for increased security.
        .disjoint_query_paths(true)
        // Closes the connection if it's idle for this amount of time.
        .set_connection_idle_timeout(config.connection_keep_alive.into());
    let kademlia = Kademlia::with_config(our_peer_id, memory_store, kademlia_config);

    // Protocol version and agent version are separate to the protocol ID for the Identify behavior.
    // See https://github.com/libp2p/specs/tree/master/identify for further details.
    let protocol_version = format!("/casper/{}", chainspec.protocol_config.version);
    let agent_version = format!("{}/{}", env!("CARGO_PKG_NAME"), env!("CARGO_PKG_VERSION"));
    let identify = Identify::new(protocol_version, agent_version, our_public_key);

    (kademlia, identify)
}
