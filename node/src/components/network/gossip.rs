//! This module is home to types/functions related to using libp2p's `GossipSub` behavior, used for
//! gossiping data to subscribed peers.

use std::fmt::{self, Display, Formatter};

use libp2p::{
    core::{ProtocolName, PublicKey},
    gossipsub::{
        Gossipsub, GossipsubConfigBuilder, MessageAuthenticity, MessageId, Topic, ValidationMode,
    },
    PeerId,
};
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};

use super::{Config, Error, ProtocolId};
use crate::{
    components::chainspec_loader::Chainspec,
    crypto::hash::{hash, Digest},
    types::Deploy,
};

/// The inner portion of the `ProtocolId` for the gossip behavior.  A standard prefix and suffix
/// will be applied to create the full protocol name.
const PROTOCOL_NAME_INNER: &str = "validator/gossip";

/// Gossiping topic used by `broadcast` functionality.
pub(super) static BROADCAST_TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("broadcast".into()));

/// Gossiping topic used by `GossipRequest<Deploy>` functionality.
pub(super) static GOSSIP_DEPLOY_TOPIC: Lazy<Topic> = Lazy::new(|| Topic::new("broadcast".into()));

/// A gossip object that can be serialized into/deserialized from a wire format.
///
/// The wireformat itself based on bincode serialization of this type, prefixed with its object hash
/// (the hash of the wrapped, deserialized type, **not** the hash of the serialized bytes).
#[derive(Debug, Deserialize, Serialize)]
pub(super) enum GossipObject {
    /// Twice-encoded legacy payload, used for broadcasting. To be removed soon.
    ///
    /// The payload is encoded twice to avoid the need for a type parameter on `GossipMessage`.
    LegacyPayload(Vec<u8>),
    /// An encoded deploy.
    Deploy(Box<Deploy>),
}

impl Display for GossipObject {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            GossipObject::LegacyPayload(inner) => {
                write!(
                    f,
                    "gossip message [legacy/broadcast]: {} bytes long",
                    inner.len()
                )
            }
            GossipObject::Deploy(inner) => {
                write!(f, "gossip message [deploy]: {}", inner)
            }
        }
    }
}

impl GossipObject {
    /// Constructs a new gossip message from a payload.
    pub(super) fn new_from_payload<I: Serialize>(payload: &I) -> Result<Self, Error> {
        let data = bincode::serialize(payload).map_err(|error| Error::Serialization(*error))?;

        Ok(GossipObject::LegacyPayload(data))
    }

    /// Constructs a new gossip message from a deploy.
    pub(super) fn new_from_deploy(deploy: Box<Deploy>) -> Self {
        GossipObject::Deploy(deploy)
    }

    /// Returns the topic the message should be sent out on.
    pub(super) fn topic(&self) -> &'static Topic {
        match self {
            GossipObject::LegacyPayload(_) => &*BROADCAST_TOPIC,
            GossipObject::Deploy(_) => &*GOSSIP_DEPLOY_TOPIC,
        }
    }

    /// Create a message ID from gossip message.
    ///
    /// The resulting message ID is equivalent to the hash of the gossiped object.
    pub(super) fn hash(&self) -> Digest {
        match self {
            GossipObject::LegacyPayload(bytes) => {
                // TODO: This is not very efficient and should be replace by making the actual type
                // available.
                hash(bytes)
            }
            GossipObject::Deploy(deploy) => deploy.id().into_inner(),
        }
    }

    /// Create the wire-serialized representation of the gossip message.
    ///
    /// Messages are prefixed with their hash, which doubles as the message id.
    pub(super) fn serialize(&self, max_gossip_message_size: usize) -> Result<Vec<u8>, Error> {
        let mut data = self.hash().to_vec();

        bincode::serialize_into(&mut data, self).map_err(|e| Error::Serialization(*e))?;

        if data.len() > max_gossip_message_size {
            return Err(Error::MessageTooLarge {
                max_size: max_gossip_message_size as u32,
                actual_size: data.len(),
            });
        }

        Ok(data)
    }

    /// Deserializes a message from wire-serialized representation.
    ///
    /// Skips the first `Digest::LENGTH` bytes, which are the hash.
    pub(super) fn deserialize(data: &[u8]) -> Result<Self, Error> {
        if data.len() < Digest::LENGTH {
            return Err(Error::MessageTooSmall {
                actual_size: data.len(),
            });
        }

        bincode::deserialize(&data[Digest::LENGTH..]).map_err(|e| Error::Deserialization(*e))
    }
}

/// Result of a gossip object validation, to be forwarded to libp2p.
#[derive(Debug)]
pub(super) struct ObjectValidationResult {
    /// The hash of the object being validated.
    message_hash: Digest,
    /// Indicates the result of the validation.
    is_valid: bool,
}

impl ObjectValidationResult {
    /// Creates a new outcome of a object validation.
    pub(super) fn new(message_hash: Digest, is_valid: bool) -> Self {
        Self {
            message_hash,
            is_valid,
        }
    }

    /// Returns the hash of the object.
    pub(super) fn message_hash(&self) -> Digest {
        self.message_hash
    }

    /// Returns whether or not the validation came out positive.
    pub(super) fn is_valid(&self) -> bool {
        self.is_valid
    }
}

/// Grants read-only access to the `MessageId` internal bytes.
///
/// This function is necessary until libp2p is updated to a version (0.34.0 or higher) that allows
/// access to the internal bytes of `MessageId`.
pub(super) fn bytes_of_message_id(message_id: &MessageId) -> &Vec<u8> {
    // From https://rust-lang.github.io/unsafe-code-guidelines/layout/structs-and-tuples.html:
    //
    // > Single-field structs
    // > A struct with only one field has the same layout as that field.

    let ptr: *const MessageId = message_id;
    unsafe { (ptr as *const Vec<u8>).as_ref() }
        .expect("did not expect valid pointer to turn into NULL")
}

#[cfg(test)]
mod unsafe_tests {
    use libp2p::gossipsub::MessageId;

    use super::bytes_of_message_id;

    #[test]
    fn does_not_blow_up() {
        let sample_data = vec![1, 2, 3, 4, 5, 6];

        let msg_id = MessageId::from(sample_data.clone());

        assert_eq!(bytes_of_message_id(&msg_id), &sample_data);
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
        .message_id_fn(|gsm| MessageId::new(&gsm.data[0..Digest::LENGTH.min(gsm.data.len())]))
        .validation_mode(ValidationMode::Permissive) // TODO: Re-add deploy verification
        .build();
    let our_peer_id = PeerId::from(our_public_key);
    let mut gossipsub = Gossipsub::new(MessageAuthenticity::Author(our_peer_id), gossipsub_config);
    gossipsub.subscribe(BROADCAST_TOPIC.clone());
    gossipsub.subscribe(GOSSIP_DEPLOY_TOPIC.clone());
    gossipsub
}
