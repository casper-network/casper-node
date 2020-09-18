use super::validators::ValidatorIndex;
use crate::components::consensus::{
    highway_core::highway::{EvidenceError, SignedWireVote},
    traits::Context,
};
use serde::{Deserialize, Serialize};

/// Evidence that a validator is faulty.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Evidence<C: Context> {
    /// The validator produced two votes with the same sequence number.
    Equivocation(SignedWireVote<C>, SignedWireVote<C>),
}

impl<C: Context> Evidence<C> {
    // TODO: Verify whether the evidence is conclusive. Or as part of deserialization?

    /// Returns the ID of the faulty validator.
    pub(crate) fn perpetrator(&self) -> ValidatorIndex {
        match self {
            Evidence::Equivocation(vote0, _) => vote0.wire_vote.creator,
        }
    }

    /// Validates the evidence and returns `Ok(())` if it is valid.
    /// "Validation" can mean different things for different type of evidence.
    ///
    /// - For an equivocation, it checks whether the creators, sequence numbers and instance IDs of
    /// the two votes are the same.
    pub(crate) fn validate(&self) -> Result<(), EvidenceError> {
        match self {
            Evidence::Equivocation(vote1, vote2) => {
                if vote1.wire_vote.creator != vote2.wire_vote.creator {
                    return Err(EvidenceError::EquivocationDifferentCreators);
                }
                if vote1.wire_vote.seq_number != vote2.wire_vote.seq_number {
                    return Err(EvidenceError::EquivocationDifferentSeqNumbers);
                }
                if vote1.wire_vote.instance_id != vote2.wire_vote.instance_id {
                    return Err(EvidenceError::EquivocationDifferentInstances);
                }
                Ok(())
            }
        }
    }
}
