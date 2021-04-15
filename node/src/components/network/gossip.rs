//! This module is home to types/functions related to using libp2p's `GossipSub` behavior, used for
//! gossiping data to subscribed peers.

use datasize::DataSize;
use libp2p::{
    core::{ProtocolName, PublicKey},
    gossipsub::{Gossipsub, GossipsubConfigBuilder, MessageAuthenticity, Topic, ValidationMode},
    PeerId,
};
use once_cell::sync::Lazy;

use super::{Config, Error, PayloadT, ProtocolId};
use crate::types::Chainspec;

/// The inner portion of the `ProtocolId` for the gossip behavior.  A standard prefix and suffix
/// will be applied to create the full protocol name.
const PROTOCOL_NAME_INNER: &str = "validator/gossip";

pub(super) static TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("all".into()));

#[derive(DataSize, Debug)]
pub(super) struct GossipMessage(pub Vec<u8>);

impl GossipMessage {
    pub(super) fn new<P: PayloadT>(payload: &P, max_size: u32) -> Result<Self, Error> {
        let serialized_message =
            bincode::serialize(payload).map_err(|error| Error::Serialization(*error))?;

        if serialized_message.len() > max_size as usize {
            return Err(Error::MessageTooLarge {
                max_size,
                actual_size: serialized_message.len() as u64,
            });
        }

        Ok(GossipMessage(serialized_message))
    }
}

impl From<GossipMessage> for Vec<u8> {
    fn from(message: GossipMessage) -> Self {
        message.0
    }
}

/// Constructs a new libp2p behavior suitable for gossiping.
pub(super) fn new_behavior(
    config: &Config,
    chainspec: &Chainspec,
    our_public_key: PublicKey,
) -> Gossipsub {
    let protocol_id = ProtocolId::new(chainspec, PROTOCOL_NAME_INNER);
    let gossipsub_config = GossipsubConfigBuilder::new()
        .protocol_id(protocol_id.protocol_name().to_vec())
        .heartbeat_interval(config.gossip_heartbeat_interval.into())
        .max_transmit_size(config.max_gossip_message_size as usize)
        .duplicate_cache_time(config.gossip_duplicate_cache_timeout.into())
        .validation_mode(ValidationMode::Permissive)
        .build();
    let our_peer_id = PeerId::from(our_public_key);
    let mut gossipsub = Gossipsub::new(MessageAuthenticity::Author(our_peer_id), gossipsub_config);
    gossipsub.subscribe(TOPIC.clone());
    gossipsub
}
