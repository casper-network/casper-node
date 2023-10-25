use datasize::DataSize;
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::components::consensus::{traits::Context, utils::ValidatorIndex};

/// An error due to an invalid endorsement.
#[derive(Debug, Error, Eq, PartialEq)]
pub(crate) enum EndorsementError {
    #[error("The creator is not a validator.")]
    Creator,
    #[error("The creator is banned.")]
    Banned,
    #[error("The signature is invalid.")]
    Signature,
    #[error("The list of endorsements is empty.")]
    Empty,
}

/// Testimony that creator of `unit` was seen honest
/// by `endorser` at the moment of creating this endorsement.
#[derive(Clone, DataSize, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub struct Endorsement<C>
where
    C: Context,
{
    /// Unit being endorsed.
    unit: C::Hash,
    /// The validator who created and sent this endorsement.
    creator: ValidatorIndex,
}

impl<C: Context> Endorsement<C> {
    pub(crate) fn new(vhash: C::Hash, creator: ValidatorIndex) -> Self {
        Endorsement {
            unit: vhash,
            creator,
        }
    }

    /// Returns the hash of the endorsement.
    pub fn hash(&self) -> C::Hash {
        <C as Context>::hash(
            &bincode::serialize(&(self.unit, self.creator)).expect("serialize endorsement"),
        )
    }
}

mod specimen_support {
    use crate::{
        components::consensus::ClContext,
        utils::specimen::{Cache, LargestSpecimen, SizeEstimator},
    };

    use super::Endorsement;

    impl LargestSpecimen for Endorsement<ClContext> {
        fn largest_specimen<E: SizeEstimator>(estimator: &E, cache: &mut Cache) -> Self {
            Endorsement {
                unit: LargestSpecimen::largest_specimen(estimator, cache),
                creator: LargestSpecimen::largest_specimen(estimator, cache),
            }
        }
    }
}

/// Testimony that creator of `unit` was seen honest
/// by `endorser` at the moment of creating this endorsement.
#[derive(Clone, DataSize, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C::Signature: Serialize",
    deserialize = "C::Signature: Deserialize<'de>",
))]
pub struct SignedEndorsement<C>
where
    C: Context,
{
    /// Original endorsement,
    endorsement: Endorsement<C>,
    /// Original signature.
    signature: C::Signature,
}

impl<C: Context> SignedEndorsement<C> {
    pub fn new(endorsement: Endorsement<C>, signature: C::Signature) -> Self {
        SignedEndorsement {
            endorsement,
            signature,
        }
    }

    /// Returns the unit being endorsed.
    pub fn unit(&self) -> &C::Hash {
        &self.endorsement.unit
    }

    /// Returns the creator of the endorsement.
    pub fn validator_idx(&self) -> ValidatorIndex {
        self.endorsement.creator
    }

    /// Returns the signature of the endorsement.
    pub fn signature(&self) -> &C::Signature {
        &self.signature
    }

    /// Returns the hash of the endorsement.
    pub fn hash(&self) -> C::Hash {
        self.endorsement.hash()
    }
}
