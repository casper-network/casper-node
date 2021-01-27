//! This module is home to types/functions related to using libp2p's `GossipSub` behavior, used for
//! gossiping data to subscribed peers.

use libp2p::{
    core::PublicKey,
    gossipsub::{
        Gossipsub, GossipsubConfigBuilder, IdentTopic, MessageAuthenticity, ValidationMode,
    },
    PeerId,
};

use super::{Config, Error, PayloadT, ProtocolId};
use crate::components::chainspec_loader::Chainspec;

/// The inner portion of the `ProtocolId` for the gossip behavior.  A standard prefix and suffix
/// will be applied to create the full protocol name.
const PROTOCOL_NAME_INNER: &str = "validator/gossip";

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
) -> Result<(Gossipsub, IdentTopic), Error> {
    let gossipsub_config = GossipsubConfigBuilder::default()
        .protocol_id_prefix(PROTOCOL_NAME_INNER)
        .heartbeat_interval(config.gossip_heartbeat_interval.into())
        .max_transmit_size(config.max_gossip_message_size as usize)
        .duplicate_cache_time(config.gossip_duplicate_cache_timeout.into())
        .validation_mode(ValidationMode::Permissive)
        .build()
        .map_err(|error| Error::Behavior(error.to_owned()))?;
    let our_peer_id = PeerId::from(our_public_key);
    let mut gossipsub = Gossipsub::new(MessageAuthenticity::Author(our_peer_id), gossipsub_config)
        .map_err(|error| Error::Behavior(error.to_owned()))?;
    let protocol_id = ProtocolId::new(chainspec, PROTOCOL_NAME_INNER);
    let topic = IdentTopic::new(protocol_id.id());
    gossipsub.subscribe(&topic)?;
    Ok((gossipsub, topic))
}
