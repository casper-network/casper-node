use std::fmt::{self, Debug, Display, Formatter};

use datasize::DataSize;
use hex_fmt::HexFmt;
#[cfg(test)]
use multihash::Multihash;
use once_cell::sync::Lazy;
#[cfg(test)]
use rand::{Rng, RngCore};
use serde::{de::Error as SerdeError, Deserialize, Deserializer, Serialize, Serializer};

#[cfg(test)]
use crate::testing::TestRng;
use crate::{rpcs::docs::DocExample, tls::KeyFingerprint};

/// The network identifier for a node.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, DataSize)]
pub enum NodeId {
    Tls(KeyFingerprint),
}

impl NodeId {
    /// Generates a random instance using a `TestRng`.
    #[cfg(test)]
    pub(crate) fn random(rng: &mut TestRng) -> Self {
        Self::random_tls(rng)
    }

    /// Generates a random Tls instance using a `TestRng`.
    #[cfg(test)]
    pub(crate) fn random_tls(rng: &mut TestRng) -> Self {
        NodeId::Tls(rng.gen())
    }

    /// Returns the raw bytes of the underlying hash of the ID, if there is any.
    #[inline]
    pub fn hash_bytes(&self) -> Option<&[u8]> {
        if let NodeId::Tls(sha256) = self {
            Some(sha256.as_ref())
        } else {
            unreachable!()
        }
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
            let helper = match self {
                NodeId::Tls(key_fingerprint) => {
                    NodeIdAsString::Tls(hex::encode(key_fingerprint.as_ref()))
                }
            };
            return helper.serialize(serializer);
        }

        let helper = match self {
            NodeId::Tls(key_fingerprint) => NodeIdAsBytes::Tls(*key_fingerprint),
        };
        helper.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for NodeId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let helper = NodeIdAsString::deserialize(deserializer)?;
            match helper {
                NodeIdAsString::Tls(hex_value) => {
                    let bytes = hex::decode(hex_value).map_err(D::Error::custom)?;
                    if bytes.len() != KeyFingerprint::LENGTH {
                        return Err(SerdeError::custom("wrong length"));
                    }
                    let mut array = [0_u8; KeyFingerprint::LENGTH];
                    array.copy_from_slice(bytes.as_slice());
                    return Ok(NodeId::Tls(KeyFingerprint::from(array)));
                }
            }
        }

        let helper = NodeIdAsBytes::deserialize(deserializer)?;
        match helper {
            NodeIdAsBytes::Tls(key_fingerprint) => Ok(NodeId::Tls(key_fingerprint)),
        }
    }
}

static NODE_ID: Lazy<NodeId> =
    Lazy::new(|| NodeId::Tls(KeyFingerprint::from([1u8; KeyFingerprint::LENGTH])));

impl DocExample for NodeId {
    fn doc_example() -> &'static Self {
        &*NODE_ID
    }
}

impl Debug for NodeId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            NodeId::Tls(key_fingerprint) => write!(
                formatter,
                "NodeId::Tls({})",
                HexFmt(key_fingerprint.as_ref())
            ),
        }
    }
}

impl Display for NodeId {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            NodeId::Tls(key_fingerprint) => {
                write!(formatter, "tls:{:10}", HexFmt(key_fingerprint.as_ref()))
            }
        }
    }
}

impl From<KeyFingerprint> for NodeId {
    fn from(id: KeyFingerprint) -> Self {
        NodeId::Tls(id)
    }
}

#[cfg(test)]
impl From<[u8; KeyFingerprint::LENGTH]> for NodeId {
    fn from(raw_bytes: [u8; KeyFingerprint::LENGTH]) -> Self {
        NodeId::Tls(KeyFingerprint::from(raw_bytes))
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn serde_roundtrip_tls() {
        let mut rng = crate::new_rng();
        let node_id = NodeId::random_tls(&mut rng);
        let serialized = bincode::serialize(&node_id).unwrap();
        let decoded = bincode::deserialize(&serialized).unwrap();
        assert_eq!(node_id, decoded);
    }

    #[test]
    fn json_roundtrip_tls() {
        let mut rng = crate::new_rng();
        let node_id = NodeId::random_tls(&mut rng);
        let json_string = serde_json::to_string_pretty(&node_id).unwrap();
        let decoded = serde_json::from_str(&json_string).unwrap();
        assert_eq!(node_id, decoded);
    }
}
