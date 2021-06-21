//! Common types used across multiple components.

pub(crate) mod appendable_block;
mod block;
pub mod chainspec;
mod deploy;
mod exit_code;
mod item;
pub mod json_compatibility;
mod node_config;
mod node_id;
mod peers_map;
mod status_feed;
mod timestamp;

use std::{fmt::Display, ops::Deref};

use rand::{CryptoRng, RngCore};
#[cfg(not(test))]
use rand_chacha::ChaCha20Rng;

pub use block::{
    json_compatibility::JsonBlock, Block, BlockBody, BlockHash, BlockHeader, BlockSignatures,
    BlockValidationError, FinalitySignature,
};
pub(crate) use block::{BlockByHeight, BlockHeaderWithMetadata, BlockPayload, FinalizedBlock};
pub(crate) use chainspec::ActivationPoint;
pub use chainspec::Chainspec;
pub use datasize::DataSize;
pub use deploy::{
    Approval, Deploy, DeployHash, DeployHeader, DeployMetadata, DeployOrTransferHash,
    DeployValidationFailure, Error as DeployError, ExcessiveSizeError as ExcessiveSizeDeployError,
};
pub use exit_code::ExitCode;
pub use item::{Item, Tag};
pub use node_config::NodeConfig;
pub(crate) use node_id::NodeId;
pub use peers_map::PeersMap;
use serde::{Deserialize, Serialize};
pub use status_feed::{ChainspecInfo, GetStatusResult, StatusFeed};
pub use timestamp::{TimeDiff, Timestamp};

/// An object-safe RNG trait that requires a cryptographically strong random number generator.
pub trait CryptoRngCore: CryptoRng + RngCore {}

impl<T> CryptoRngCore for T where T: CryptoRng + RngCore + ?Sized {}

/// The cryptographically secure RNG used throughout the node.
#[cfg(not(test))]
pub type NodeRng = ChaCha20Rng;

/// The RNG used throughout the node for testing.
#[cfg(test)]
pub type NodeRng = crate::testing::TestRng;

/// An in-memory object that can possibly be shared with user parts of the system.
///
/// In general, this should only be used for immutable, content-addressed objects.
///
/// This type exists solely to switch between `Box` and `Arc` based behavior, future updates should
/// deprecate this in favor of using `Arc`s directly or turning `LoadedItem` into a newtype.
#[derive(Clone, DataSize, Debug, Eq, Ord, PartialEq, PartialOrd)]
pub enum SharedObject<T> {
    /// An owned copy of the object.
    Owned(Box<T>),
}

impl<T> Deref for SharedObject<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        match self {
            SharedObject::Owned(obj) => &*obj,
        }
    }
}

impl<T> AsRef<[u8]> for SharedObject<T>
where
    T: AsRef<[u8]>,
{
    fn as_ref(&self) -> &[u8] {
        match self {
            SharedObject::Owned(obj) => <T as AsRef<[u8]>>::as_ref(obj),
        }
    }
}

impl<T> SharedObject<T> {
    /// Creates a new owned instance of the object.
    #[inline]
    pub fn owned(inner: T) -> Self {
        SharedObject::Owned(Box::new(inner))
    }

    /// Converts a loaded object into an instance of `T`.
    ///
    /// May clone the object as a result. This method should not be used in new code, it exists
    /// solely to bridge old interfaces with the `LoadedItem`.
    #[inline]
    pub(crate) fn into_inner(self) -> T {
        match self {
            SharedObject::Owned(inner) => *inner,
        }
    }
}

impl<T> Display for SharedObject<T>
where
    T: Display,
{
    #[inline]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SharedObject::Owned(inner) => inner.fmt(f),
        }
    }
}

impl<T> Serialize for SharedObject<T>
where
    T: Serialize,
{
    #[inline]
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            SharedObject::Owned(inner) => inner.serialize(serializer),
        }
    }
}

impl<'de, T> Deserialize<'de> for SharedObject<T>
where
    T: Deserialize<'de>,
{
    #[inline]
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        T::deserialize(deserializer).map(SharedObject::owned)
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use serde::{de::DeserializeOwned, Serialize};

    use super::{Deploy, SharedObject};

    // TODO: Import fixed serialization settings from `small_network::message_pack_format` as soon
    //       as merged, instead of using these `rmp_serde` helper functions.
    #[inline]
    fn serialize<T: Serialize>(value: &T) -> Vec<u8> {
        rmp_serde::to_vec(value).expect("could not serialize value")
    }

    #[inline]
    fn deserialize<T: DeserializeOwned>(raw: &[u8]) -> T {
        rmp_serde::from_read(Cursor::new(raw)).expect("could not deserialize value")
    }

    #[test]
    fn loaded_item_for_bytes_deserializes_like_bytevec() {
        // Construct an example payload that is reasonably realistic.
        let mut rng = crate::new_rng();
        let deploy = Deploy::random(&mut rng);
        let payload = bincode::serialize(&deploy).expect("could not serialize deploy");

        // Realistic payload inside a `GetRequest`.
        let loaded_item_owned = SharedObject::owned(payload.clone());
        // TODO: Shared variant.

        // Check all serialize the same.
        let serialized = serialize(&payload);
        assert_eq!(serialized, serialize(&loaded_item_owned));

        // Ensure we can deserialize a loaded item payload.
        let deserialized: SharedObject<Vec<u8>> = deserialize(&serialized);

        assert_eq!(payload, deserialized.into_inner());
    }
}
