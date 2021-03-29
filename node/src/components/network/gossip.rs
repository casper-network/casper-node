//! This module is home to types/functions related to using libp2p's `GossipSub` behavior, used for
//! gossiping data to subscribed peers.

use datasize::DataSize;
use libp2p::{
    core::PublicKey,
    gossipsub::{
        Gossipsub, GossipsubConfigBuilder, IdentTopic, MessageAuthenticity, ValidationMode,
    },
    PeerId,
};
use once_cell::sync::Lazy;

use super::{Config, Error, PayloadT};
use crate::types::Chainspec;

pub(super) static TOPIC: Lazy<IdentTopic> = Lazy::new(|| IdentTopic::new("all".to_string()));

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
    _chainspec: &Chainspec,
    our_public_key: PublicKey,
) -> Gossipsub {
    let gossipsub_config = GossipsubConfigBuilder::default()
        // TODO - consider not using the default protocol ID prefix.
        // .protocol_id(ProtocolId::new(chainspec, "validator/gossip").protocol_name().to_vec())
        .heartbeat_interval(config.gossip_heartbeat_interval.into())
        .max_transmit_size(config.max_gossip_message_size as usize)
        .duplicate_cache_time(config.gossip_duplicate_cache_timeout.into())
        .validation_mode(ValidationMode::Permissive)
        .build()
        .unwrap_or_else(|error| panic!("should construct gossipsub config: {}", error));
    let our_peer_id = PeerId::from(our_public_key);
    // TODO - remove `expect`
    let mut gossipsub = Gossipsub::new(MessageAuthenticity::Author(our_peer_id), gossipsub_config)
        .expect("should construct a new gossipsub behavior");
    // TODO - remove `expect`
    gossipsub
        .subscribe(&*TOPIC)
        .expect("should subscribe to topic");
    gossipsub
}
