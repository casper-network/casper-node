//! This module is home to types/functions related to using libp2p's `GossipSub` behavior, used for
//! gossiping data to subscribed peers.

use libp2p::{
    core::{ProtocolName, PublicKey},
    gossipsub::{Gossipsub, GossipsubConfigBuilder, MessageAuthenticity, Topic, ValidationMode},
    PeerId,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use super::{Config, Error, ProtocolId};
use crate::{components::chainspec_loader::Chainspec, types::Deploy};

/// The inner portion of the `ProtocolId` for the gossip behavior.  A standard prefix and suffix
/// will be applied to create the full protocol name.
const PROTOCOL_NAME_INNER: &str = "validator/gossip";

const ASSUMED_SERIALIZATION_OVERHEAD: usize = 8;

/// Gossiping topic used by `broadcast` functionality.
pub(super) static BROADCAST_TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("broadcast".into()));

/// Gossiping topic used by `GossipRequest<Deploy>` functionality.
pub(super) static GOSSIP_DEPLOY_TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("broadcast".into()));

#[derive(Debug, Deserialize, Serialize)]
pub(super) enum GossipMessage {
    /// Twice-encoded legacy payload, used for broadcasting. To be removed soon.
    LegacyPayload(Vec<u8>),
    /// An encoded deploy.
    Deploy(Deploy),
}

fn serialize_gossip_message<T: Serialize>(item: &T, max_size: u32) -> Result<Vec<u8>, Error> {
    let data = bincode::serialize(item).map_err(|error| Error::Serialization(*error))?;

    // We add 8 bytes of potential tag serialization overhead.
    if data.len() + ASSUMED_SERIALIZATION_OVERHEAD > max_size as usize {
        return Err(Error::MessageTooLarge {
            max_size: max_size - ASSUMED_SERIALIZATION_OVERHEAD as u32,
            actual_size: data.len(),
        });
    }
    Ok(data)
}

impl GossipMessage {
    pub(super) fn new_from_payload<I: Serialize>(
        payload: &I,
        max_size: u32,
    ) -> Result<Self, Error> {
        serialize_gossip_message(payload, max_size).map(GossipMessage::LegacyPayload)
    }

    pub(super) fn new_from_deploy(deploy: Deploy) -> Self {
        GossipMessage::Deploy(deploy)
    }

    /// Returns the topic the message should be sent out on.
    pub(super) fn topic(&self) -> &'static Topic {
        match self {
            GossipMessage::LegacyPayload(_) => &*BROADCAST_TOPIC,
            GossipMessage::Deploy(_) => &*GOSSIP_DEPLOY_TOPIC,
        }
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
