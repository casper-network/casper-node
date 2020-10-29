use serde::{Deserialize, Serialize};
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

/// Testimony that creator of `vote` was seen honest
/// by `endorser` at the moment of creating this endorsement.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct Endorsement<C: Context> {
    /// Vote being endorsed.
    vote: C::Hash,
    /// The validator who created and sent this endorsement.
    creator: ValidatorIndex,
    /// Original signature.
    signature: C::Signature,
}

impl<C: Context> Endorsement<C> {
    pub fn new(vote: C::Hash, creator: ValidatorIndex, signature: C::Signature) -> Self {
        Endorsement {
            vote,
            creator,
            signature,
        }
    }

    pub(crate) fn validator_idx(&self) -> ValidatorIndex {
        self.creator
    }

    pub(crate) fn hash(&self) -> C::Hash {
        <C as Context>::hash(
            &bincode::serialize(&(self.vote, self.creator)).expect("serialize endorsement"),
        )
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) struct Endorsements<C: Context> {
    pub(crate) vote: C::Hash,
    pub(crate) endorsers: Vec<(ValidatorIndex, C::Signature)>,
}

impl<C: Context> Endorsements<C> {
    pub fn new<I: IntoIterator<Item = Endorsement<C>>>(endorsements: I) -> Self {
        let mut iter = endorsements.into_iter().peekable();
        let vote: C::Hash = iter.peek().expect("non-empty iter").vote;
        let endorsers = iter
            .map(|e| {
                assert_eq!(e.vote, vote, "endorsements for different votes.");
                (e.creator, e.signature)
            })
            .collect();
        Endorsements { vote, endorsers }
    }

    /// Returns hash of the endorsed vode.
    pub fn vote(&self) -> &C::Hash {
        &self.vote
    }

    /// Returns an iterator over validator indexes that endorsed the `vote`.
    pub fn validator_ids(&self) -> impl Iterator<Item = &ValidatorIndex> {
        self.endorsers.iter().map(|(v, _)| v)
    }
}
