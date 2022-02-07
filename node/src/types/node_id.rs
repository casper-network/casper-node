use std::fmt::{self, Debug, Display, Formatter};

use datasize::DataSize;
use hex_fmt::HexFmt;
use once_cell::sync::Lazy;
#[cfg(test)]
use rand::Rng;
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

#[cfg(test)]
use crate::testing::TestRng;
use crate::{rpcs::docs::DocExample, tls::KeyFingerprint};

/// The network identifier for a node.
///
/// A node's ID is derived from the fingerprint of its TLS certificate.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, DataSize)]
pub struct NodeId(KeyFingerprint);

impl NodeId {
    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        Self(rng.gen())
    }

    /// Returns the raw bytes of the underlying hash of the ID.
    #[inline]
    pub fn hash_bytes(&self) -> &[u8] {
        self.0.as_ref()
    }
}

/// Used to serialize and deserialize `NodeID` where the (de)serializer isn't a human-readable type.
#[derive(Serialize, Deserialize)]
enum NodeIdAsBytes {
    Tls(KeyFingerprint),
}

/// Used to serialize and deserialize `NodeID` where the (de)serializer is a human-readable type.
#[derive(Serialize, Deserialize)]
enum NodeIdAsString {
    Tls(String),
}

impl Serialize for NodeId {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        if serializer.is_human_readable() {
            NodeIdAsString::Tls(base16::encode_lower(&self.0)).serialize(serializer)
        } else {
            NodeIdAsBytes::Tls(self.0).serialize(serializer)
        }
    }
}

impl<'de> Deserialize<'de> for NodeId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let NodeIdAsString::Tls(hex_value) = NodeIdAsString::deserialize(deserializer)?;

            let bytes = base16::decode(hex_value.as_bytes()).map_err(D::Error::custom)?;
            if bytes.len() != KeyFingerprint::LENGTH {
                return Err(SerdeError::custom("wrong length"));
            }
            let mut array = [0_u8; KeyFingerprint::LENGTH];
            array.copy_from_slice(bytes.as_slice());

            Ok(NodeId(KeyFingerprint::from(array)))
        } else {
            let NodeIdAsBytes::Tls(key_fingerprint) = NodeIdAsBytes::deserialize(deserializer)?;
            Ok(NodeId(key_fingerprint))
        }
    }
}

static NODE_ID: Lazy<NodeId> =
    Lazy::new(|| NodeId(KeyFingerprint::from([1u8; KeyFingerprint::LENGTH])));

impl DocExample for NodeId {
    fn doc_example() -> &'static Self {
        &*NODE_ID
    }
}

impl Debug for NodeId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "NodeId({})", base16::encode_lower(&self.0))
    }
}

impl Display for NodeId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "tls:{:10}", HexFmt(&self.0))
    }
}

impl From<KeyFingerprint> for NodeId {
    fn from(id: KeyFingerprint) -> Self {
        NodeId(id)
    }
}

#[cfg(test)]
impl From<[u8; KeyFingerprint::LENGTH]> for NodeId {
    fn from(raw_bytes: [u8; KeyFingerprint::LENGTH]) -> Self {
        NodeId(KeyFingerprint::from(raw_bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EXAMPLE_HASH_RAW: [u8; 64] = [
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
        0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c,
        0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b,
        0x3c, 0x3d, 0x3e, 0x3f,
    ];

    #[test]
    fn serde_roundtrip_tls() {
        let mut rng = crate::new_rng();
        let node_id = NodeId::random(&mut rng);
        let serialized = bincode::serialize(&node_id).unwrap();
        let decoded = bincode::deserialize(&serialized).unwrap();
        assert_eq!(node_id, decoded);
    }

    #[test]
    fn bincode_known_specimen() {
        let node_id = NodeId::from(EXAMPLE_HASH_RAW);
        let serialized = bincode::serialize(&node_id).unwrap();

        // The bincode representation is a 4 byte tag of all zeros, followed by the hash bytes.
        let expected: [u8; 68] = [
            0x00, 0x00, 0x00, 0x00, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09,
            0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
            0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f, 0x20, 0x21, 0x22, 0x23, 0x24, 0x25,
            0x26, 0x27, 0x28, 0x29, 0x2a, 0x2b, 0x2c, 0x2d, 0x2e, 0x2f, 0x30, 0x31, 0x32, 0x33,
            0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a, 0x3b, 0x3c, 0x3d, 0x3e, 0x3f,
        ];

        assert_eq!(&expected[..], serialized.as_slice());
    }

    #[test]
    fn json_known_specimen() {
        let node_id = NodeId::from(EXAMPLE_HASH_RAW);
        let json_string = serde_json::to_string_pretty(&node_id).unwrap();

        let expected = "{\n  \"Tls\": \"000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f303132333435363738393a3b3c3d3e3f\"\n}";
        assert_eq!(expected, json_string.as_str());
    }

    #[test]
    fn msgpack_default_settings_known_specimen() {
        let node_id = NodeId::from(EXAMPLE_HASH_RAW);

        let serialized = rmp_serde::to_vec(&node_id).unwrap();

        let expected: [u8; 132] = [
            129, 0, 217, 128, 48, 48, 48, 49, 48, 50, 48, 51, 48, 52, 48, 53, 48, 54, 48, 55, 48,
            56, 48, 57, 48, 97, 48, 98, 48, 99, 48, 100, 48, 101, 48, 102, 49, 48, 49, 49, 49, 50,
            49, 51, 49, 52, 49, 53, 49, 54, 49, 55, 49, 56, 49, 57, 49, 97, 49, 98, 49, 99, 49,
            100, 49, 101, 49, 102, 50, 48, 50, 49, 50, 50, 50, 51, 50, 52, 50, 53, 50, 54, 50, 55,
            50, 56, 50, 57, 50, 97, 50, 98, 50, 99, 50, 100, 50, 101, 50, 102, 51, 48, 51, 49, 51,
            50, 51, 51, 51, 52, 51, 53, 51, 54, 51, 55, 51, 56, 51, 57, 51, 97, 51, 98, 51, 99, 51,
            100, 51, 101, 51, 102,
        ];

        assert_eq!(serialized, expected);
    }

    #[test]
    fn json_roundtrip_tls() {
        let mut rng = crate::new_rng();
        let node_id = NodeId::random(&mut rng);
        let json_string = serde_json::to_string_pretty(&node_id).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(node_id, decoded);
    }
}
