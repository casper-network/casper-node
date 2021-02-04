use std::{
    fmt::{Debug, Display},
    hash::Hash,
};

use datasize::DataSize;
use serde::{de::DeserializeOwned, Serialize};

use crate::NodeRng;

pub trait NodeIdT: Clone + Display + Debug + Send + Eq + Hash + DataSize + 'static {}
impl<I> NodeIdT for I where I: Clone + Display + Debug + Send + Eq + Hash + DataSize + 'static {}

/// A validator identifier.
pub(crate) trait ValidatorIdT: Eq + Ord + Clone + Debug + Hash + Send + DataSize {}
impl<VID> ValidatorIdT for VID where VID: Eq + Ord + Clone + Debug + Hash + Send + DataSize {}

/// The consensus value type, e.g. a list of transactions.
pub(crate) trait ConsensusValueT:
    Eq + Clone + Debug + Hash + Serialize + DeserializeOwned + Send + DataSize
{
    /// Returns whether the consensus value needs validation.
    fn needs_validation(&self) -> bool;
}

/// A hash, as an identifier for a block or unit.
pub(crate) trait HashT:
    Eq + Ord + Copy + Clone + DataSize + Debug + Display + Hash + Serialize + DeserializeOwned + Send
{
}
impl<H> HashT for H where
    H: Eq
        + Ord
        + Copy
        + Clone
        + DataSize
        + Debug
        + Display
        + Hash
        + Serialize
        + DeserializeOwned
        + Send
{
}

/// A validator's secret signing key.
pub(crate) trait ValidatorSecret: Send + DataSize {
    type Hash: DataSize;

    type Signature: Eq + PartialEq + Clone + Debug + Hash + Serialize + DeserializeOwned + DataSize;

    fn sign(&self, hash: &Self::Hash, rng: &mut NodeRng) -> Self::Signature;
}

/// The collection of types the user can choose for cryptography, IDs, transactions, etc.
// TODO: These trait bounds make `#[derive(...)]` work for types with a `C: Context` type
// parameter. Split this up or replace the derives with explicit implementations.
pub(crate) trait Context: Clone + DataSize + Debug + Eq + Ord + Hash + Send {
    /// The consensus value type, e.g. a list of transactions.
    type ConsensusValue: ConsensusValueT;
    /// Unique identifiers for validators.
    type ValidatorId: ValidatorIdT;
    /// A validator's secret signing key.
    type ValidatorSecret: ValidatorSecret<Hash = Self::Hash, Signature = Self::Signature>;
    /// A signature type.
    type Signature: Copy
        + Clone
        + Debug
        + Eq
        + Hash
        + Serialize
        + DeserializeOwned
        + Send
        + DataSize;
    /// Unique identifiers for units.
    type Hash: HashT;
    /// The ID of a consensus protocol instance.
    type InstanceId: HashT;

    fn hash(data: &[u8]) -> Self::Hash;

    fn verify_signature(
        hash: &Self::Hash,
        public_key: &Self::ValidatorId,
        signature: &<Self::ValidatorSecret as ValidatorSecret>::Signature,
    ) -> bool;
}
