use std::{
    fmt::{self, Debug, Display, Formatter},
    net::SocketAddr,
    sync::Arc,
};

use casper_types::{ProtocolVersion, PublicKey, SecretKey, Signature};
use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::crypto;

use super::counting_format::ConnectionId;

/// The default protocol version to use in absence of one in the protocol version field.
#[inline]
fn default_protocol_version() -> ProtocolVersion {
    ProtocolVersion::V1_0_0
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub enum Message<P> {
    Handshake {
        /// Network we are connected to.
        network_name: String,
        /// The public address of the node connecting.
        public_addr: SocketAddr,
        /// Protocol version the node is speaking.
        #[serde(default = "default_protocol_version")]
        protocol_version: ProtocolVersion,
        /// A self-signed certificate indicating validator status.
        #[serde(default)]
        consensus_certificate: Option<ConsensusCertificate>,
    },
    Payload(P),
}

impl<P: Payload> Message<P> {
    /// Classifies a message based on its payload.
    #[inline]
    pub(super) fn classify(&self) -> MessageKind {
        match self {
            Message::Handshake { .. } => MessageKind::Protocol,
            Message::Payload(payload) => payload.classify(),
        }
    }

    /// Returns the incoming resource estimate of the payload.
    #[inline]
    pub(super) fn payload_incoming_resource_estimate(&self) -> u32 {
        match self {
            Message::Handshake { .. } => 0,
            Message::Payload(payload) => payload.incoming_resource_estimate(),
        }
    }
}

/// A pair of secret keys used by consensus.
pub(super) struct ConsensusKeyPair {
    secret_key: Arc<SecretKey>,
    public_key: PublicKey,
}

impl ConsensusKeyPair {
    /// Creates a new key pair for consensus signing.
    pub(super) fn new(secret_key: Arc<SecretKey>, public_key: PublicKey) -> Self {
        Self {
            secret_key,
            public_key,
        }
    }

    /// Sign a value using this keypair.
    fn sign<T: AsRef<[u8]>>(&self, value: T) -> Signature {
        crypto::sign(value, &self.secret_key, &self.public_key)
    }
}

/// Certificate used to indicate that the peer is a validator using the specified public key.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct ConsensusCertificate {
    public_key: PublicKey,
    signature: Signature,
}

impl ConsensusCertificate {
    /// Creates a new consensus certificate from a connection ID and key pair.
    pub(super) fn create(connection_id: ConnectionId, key_pair: &ConsensusKeyPair) -> Self {
        let signature = key_pair.sign(connection_id.as_bytes());
        ConsensusCertificate {
            public_key: key_pair.public_key.clone(),
            signature,
        }
    }

    /// Validates a certificate, returning a `PublicKey` if valid.
    pub(super) fn validate(self, connection_id: ConnectionId) -> Result<PublicKey, crypto::Error> {
        crypto::verify(connection_id.as_bytes(), &self.signature, &self.public_key)?;
        Ok(self.public_key)
    }
}

impl Display for ConsensusCertificate {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "key:{}", self.public_key)
    }
}

impl<P: Display> Display for Message<P> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Message::Handshake {
                network_name,
                public_addr,
                protocol_version,
                consensus_certificate,
            } => {
                write!(
                    f,
                    "handshake: {}, public addr: {}, protocol_version: {}, consensus_certificate: ",
                    network_name, public_addr, protocol_version
                )?;

                if let Some(cert) = consensus_certificate {
                    write!(f, "{}", cert)
                } else {
                    f.write_str("-")
                }
            }
            Message::Payload(payload) => write!(f, "payload: {}", payload),
        }
    }
}

/// A classification system for networking messages.
#[derive(Copy, Clone, Debug)]
pub enum MessageKind {
    /// Non-payload messages, like handshakes.
    Protocol,
    /// Messages directly related to consensus.
    Consensus,
    /// Deploys being gossiped.
    DeployGossip,
    /// Addresses being gossiped.
    AddressGossip,
    /// Deploys being transferred directly (via requests).
    DeployTransfer,
    /// Blocks for finality signatures being transferred directly (via requests and other means).
    BlockTransfer,
    /// Any other kind of payload (or missing classification).
    Other,
}

impl Display for MessageKind {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            MessageKind::Protocol => f.write_str("protocol"),
            MessageKind::Consensus => f.write_str("consensus"),
            MessageKind::DeployGossip => f.write_str("deploy_gossip"),
            MessageKind::AddressGossip => f.write_str("address_gossip"),
            MessageKind::DeployTransfer => f.write_str("deploy_transfer"),
            MessageKind::BlockTransfer => f.write_str("block_transfer"),
            MessageKind::Other => f.write_str("other"),
        }
    }
}

/// Network message payload.
///
/// Payloads are what is transferred across the network outside of control messages from the
/// networking component itself.
pub trait Payload:
    Serialize + DeserializeOwned + Clone + Debug + Display + Send + Sync + 'static
{
    /// Classifies the payload based on its contents.
    fn classify(&self) -> MessageKind;

    /// The penalty for resource usage of a message to be applied when processed as incoming.
    fn incoming_resource_estimate(&self) -> u32 {
        0
    }
}

#[cfg(test)]
// We use a variety of weird names in these tests.
#[allow(non_camel_case_types)]
mod tests {
    use std::net::SocketAddr;

    use casper_types::ProtocolVersion;
    use serde::{de::DeserializeOwned, Deserialize, Serialize};

    use crate::protocol;

    use super::Message;

    /// Version 1.0.0 network level message.
    ///
    /// Note that the message itself may go out of sync over time as `protocol::Message` changes.
    /// The test further below ensures that the handshake is accurate in the meantime.
    #[derive(Clone, Debug, Deserialize, Serialize)]
    pub enum V1_0_0_Message {
        Handshake {
            /// Network we are connected to.
            network_name: String,
            /// The public address of the node connecting.
            public_address: SocketAddr,
        },
        Payload(protocol::Message),
    }

    /// A "conserved" version 1.0.0 handshake.
    ///
    /// NEVER CHANGE THIS CONSTANT TO MAKE TESTS PASS, AS IT IS BASED ON MAINNET DATA.
    const V1_0_0_HANDSHAKE: &[u8] = &[
        129, 0, 146, 178, 115, 101, 114, 105, 97, 108, 105, 122, 97, 116, 105, 111, 110, 45, 116,
        101, 115, 116, 177, 49, 50, 46, 51, 52, 46, 53, 54, 46, 55, 56, 58, 49, 50, 51, 52, 54,
    ];

    // Note: MessagePack messages can be visualized using the message pack visualizer at
    // https://sugendran.github.io/msgpack-visualizer/. Rust arrays can be copy&pasted and converted
    // to base64 using the following one-liner: `import base64; base64.b64encode(bytes([129, 0,
    // ...]))`

    // It is very important to note that different versions of the message pack codec crate set the
    // human-readable flag in a different manner. Thus the V1.0.0 handshake can be serialized in two
    // different ways, with "human readable" enabled and without.
    //
    // Our V1.0.0 protocol uses the "human readable" enabled version, they key difference being that
    // the `SocketAddr` is encoded as a string instead of a two-item array.

    /// A pseudo-1.0.0 handshake, where the serde human readable flag has been changed due to an
    /// `rmp` version mismatch.
    const BROKEN_V1_0_0_HANDSHAKE: &[u8] = &[
        129, 0, 146, 178, 115, 101, 114, 105, 97, 108, 105, 122, 97, 116, 105, 111, 110, 45, 116,
        101, 115, 116, 129, 0, 146, 148, 12, 34, 56, 78, 205, 48, 58,
    ];

    /// Serialize a message using the standard serialization method for handshakes.
    fn serialize_message<M: Serialize>(msg: &M) -> Vec<u8> {
        // The actual serialization/deserialization code can be found at
        // https://github.com/carllerche/tokio-serde/blob/f3c3d69ce049437973468118c9d01b46e0b1ade5/src/lib.rs#L426-L450

        rmp_serde::to_vec(&msg).expect("handshake serialization failed")
    }

    /// Deserialize a message using the standard deserialization method for handshakes.
    fn deserialize_message<M: DeserializeOwned>(serialized: &[u8]) -> M {
        rmp_serde::from_read(std::io::Cursor::new(&serialized))
            .expect("handshake deserialization failed")
    }

    /// Given a message `from` of type `F`, serializes it, then deserializes it as `T`.
    fn roundtrip_message<F, T>(from: &F) -> T
    where
        F: Serialize,
        T: DeserializeOwned,
    {
        let serialized = serialize_message(from);
        deserialize_message(&serialized)
    }

    // This test ensure that the serialization of the `V_1_0_0_Message` has not changed and that the
    // serialization/deserialization methods for message in this test are likely accurate.
    #[test]
    fn v1_0_0_handshake_is_as_expected() {
        let handshake = V1_0_0_Message::Handshake {
            network_name: "serialization-test".to_owned(),
            public_address: ([12, 34, 56, 78], 12346).into(),
        };

        let serialized = serialize_message::<V1_0_0_Message>(&handshake);

        assert_eq!(&serialized, V1_0_0_HANDSHAKE);
        assert_ne!(&serialized, BROKEN_V1_0_0_HANDSHAKE);

        let deserialized: V1_0_0_Message = deserialize_message(&serialized);

        match deserialized {
            V1_0_0_Message::Handshake {
                network_name,
                public_address,
            } => {
                assert_eq!(network_name, "serialization-test");
                assert_eq!(public_address, ([12, 34, 56, 78], 12346).into());
            }
            other => {
                panic!("did not expect {:?} as the deserialized product", other);
            }
        }
    }

    #[test]
    fn v1_0_0_can_decode_current_handshake() {
        let modern_handshake = Message::<protocol::Message>::Handshake {
            network_name: "example-handshake".to_string(),
            public_addr: ([12, 34, 56, 78], 12346).into(),
            protocol_version: ProtocolVersion::from_parts(5, 6, 7),
            // TODO: Test _with_ handshake instead.
            consensus_certificate: None,
        };

        let legacy_handshake: V1_0_0_Message = roundtrip_message(&modern_handshake);

        match legacy_handshake {
            V1_0_0_Message::Handshake {
                network_name,
                public_address,
            } => {
                assert_eq!(network_name, "example-handshake");
                assert_eq!(public_address, ([12, 34, 56, 78], 12346).into());
            }
            V1_0_0_Message::Payload(_) => {
                panic!("did not expect legacy handshake to deserialize to payload")
            }
        }
    }

    #[test]
    fn current_handshake_decodes_from_v1_0_0() {
        let legacy_handshake = V1_0_0_Message::Handshake {
            network_name: "example-handshake".to_string(),
            public_address: ([12, 34, 56, 78], 12346).into(),
        };

        let modern_handshake: Message<protocol::Message> = roundtrip_message(&legacy_handshake);

        match modern_handshake {
            Message::Handshake {
                network_name,
                public_addr,
                protocol_version,
                consensus_certificate,
            } => {
                assert_eq!(network_name, "example-handshake");
                assert_eq!(public_addr, ([12, 34, 56, 78], 12346).into());
                assert_eq!(protocol_version, ProtocolVersion::V1_0_0);
                assert!(consensus_certificate.is_none());
            }
            Message::Payload(_) => {
                panic!("did not expect modern handshake to deserialize to payload")
            }
        }
    }

    #[test]
    fn current_handshake_decodes_from_historic_v1_0_0() {
        let modern_handshake: Message<protocol::Message> = deserialize_message(V1_0_0_HANDSHAKE);

        match modern_handshake {
            Message::Handshake {
                network_name,
                public_addr,
                protocol_version,
                consensus_certificate,
            } => {
                assert_eq!(network_name, "serialization-test");
                assert_eq!(public_addr, ([12, 34, 56, 78], 12346).into());
                assert_eq!(protocol_version, ProtocolVersion::V1_0_0);
                assert!(consensus_certificate.is_none());
            }
            Message::Payload(_) => {
                panic!("did not expect modern handshake to deserialize to payload")
            }
        }
    }
}
