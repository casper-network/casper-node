use std::{fmt::Debug, hash::Hash};

use serde::{de::DeserializeOwned, Serialize};

use crate::types::ProtoBlock;

pub(crate) trait NodeIdT: Clone + Debug + Send + 'static {}
impl<I> NodeIdT for I where I: Clone + Debug + Send + 'static {}

/// A validator identifier.
pub(crate) trait ValidatorIdT: Eq + Ord + Clone + Debug + Hash {}
impl<VID> ValidatorIdT for VID where VID: Eq + Ord + Clone + Debug + Hash {}

/// The consensus value type, e.g. a list of transactions.
pub(crate) trait ConsensusValueT:
    Eq + Clone + Debug + Hash + Serialize + DeserializeOwned
{
    /// Returns `true` if this value should be the last one to be finalized in the current
    /// consensus instance, i.e. era.
    fn terminal(&self) -> bool;
}

impl ConsensusValueT for ProtoBlock {
    fn terminal(&self) -> bool {
        self.switch_block()
    }
}

/// A hash, as an identifier for a block or vote.
pub(crate) trait HashT:
    Eq + Ord + Clone + Debug + Hash + Serialize + DeserializeOwned
{
}
impl<H> HashT for H where H: Eq + Ord + Clone + Debug + Hash + Serialize + DeserializeOwned {}

/// A validator's secret signing key.
pub(crate) trait ValidatorSecret {
    type Hash;

    type Signature: Eq + PartialEq + Clone + Debug + Hash + Serialize + DeserializeOwned;

    fn sign(&self, data: &Self::Hash) -> Self::Signature;
}

/// The collection of types the user can choose for cryptography, IDs, transactions, etc.
// TODO: These trait bounds make `#[derive(...)]` work for types with a `C: Context` type
// parameter. Split this up or replace the derives with explicit implementations.
pub(crate) trait Context: Clone + Debug + Eq + Ord + Hash {
    /// The consensus value type, e.g. a list of transactions.
    type ConsensusValue: ConsensusValueT;
    /// Unique identifiers for validators.
    type ValidatorId: ValidatorIdT;
    /// A validator's secret signing key.
    type ValidatorSecret: ValidatorSecret<Hash = Self::Hash, Signature = Self::Signature>;
    /// A signature type.
    type Signature: Clone + Debug + Eq + Hash + Serialize + DeserializeOwned;
    /// Unique identifiers for votes.
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
