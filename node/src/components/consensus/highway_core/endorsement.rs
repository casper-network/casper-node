use thiserror::Error;

use crate::components::consensus::traits::Context;

use super::validators::ValidatorIndex;

/// An error due to an invalid endorsement.
#[derive(Debug, Error, PartialEq)]
pub(crate) enum EndorsementError {
    #[error("The creator is not a validator.")]
    Creator,
    #[error("The signature is invalid.")]
    Signature,
}

/// Testimony that creator of `unit` was seen honest
/// by `endorser` at the moment of creating this endorsement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct Endorsement<C: Context> {
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

    pub(crate) fn hash(&self) -> C::Hash {
        <C as Context>::hash(
            &bincode::serialize(&(self.unit, self.creator)).expect("serialize endorsement"),
        )
    }
}

/// Testimony that creator of `unit` was seen honest
/// by `endorser` at the moment of creating this endorsement.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct SignedEndorsement<C: Context> {
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

    pub(crate) fn unit(&self) -> &C::Hash {
        &self.endorsement.unit
    }

    pub(crate) fn validator_idx(&self) -> ValidatorIndex {
        self.endorsement.creator
    }

    pub(crate) fn signature(&self) -> &C::Signature {
        &self.signature
    }
}
