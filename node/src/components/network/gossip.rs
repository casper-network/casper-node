//! This module is home to types/functions related to using libp2p's `GossipSub` behavior, used for
//! gossiping data to subscribed peers.

use libp2p::{
    core::{ProtocolName, PublicKey},
    gossipsub::{Gossipsub, GossipsubConfigBuilder, MessageAuthenticity, Topic, ValidationMode},
    PeerId,
};
use once_cell::sync::Lazy;
use serde::Serialize;

use super::{Config, Error, ProtocolId};
use crate::components::chainspec_loader::Chainspec;

/// The inner portion of the `ProtocolId` for the gossip behavior.  A standard prefix and suffix
/// will be applied to create the full protocol name.
const PROTOCOL_NAME_INNER: &str = "validator/gossip";

/// Gossiping topic used by `broadcast` functionality.
pub(super) static BROADCAST_TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("broadcast".into()));

/// Gossiping topic used by `GossipRequest<Deploy>` functionality.
pub(super) static GOSSIP_DEPLOY_TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("broadcast".into()));

pub(super) struct GossipMessage {
    /// Topic to gossip to.
    pub topic: &'static Topic,
    /// Serialized data to be gossiped.
    pub data: Vec<u8>,
}

impl GossipMessage {
    pub(super) fn new<I: Serialize>(
        topic: &'static Topic,
        item: &I,
        max_size: u32,
    ) -> Result<Self, Error> {
        let data = bincode::serialize(item).map_err(|error| Error::Serialization(*error))?;

        if data.len() > max_size as usize {
            return Err(Error::MessageTooLarge {
                max_size,
                actual_size: data.len() as u64,
            });
        }

        Ok(GossipMessage { topic, data })
    }
}

impl From<GossipMessage> for Vec<u8> {
    fn from(message: GossipMessage) -> Self {
        message.data
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
        .validation_mode(ValidationMode::Permissive) // TODO: Re-add deploy verification
        .build();
    let our_peer_id = PeerId::from(our_public_key);
    let mut gossipsub = Gossipsub::new(MessageAuthenticity::Author(our_peer_id), gossipsub_config);
    gossipsub.subscribe(BROADCAST_TOPIC.clone());
    gossipsub.subscribe(GOSSIP_DEPLOY_TOPIC.clone());
    gossipsub
}
