use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::validators::ValidatorIndex;
use crate::components::consensus::{highway_core::highway::SignedWireVote, traits::Context};

/// An error due to invalid evidence.
#[derive(Debug, Error, PartialEq)]
pub(crate) enum EvidenceError {
    #[error("The creators in the equivocating votes are different.")]
    EquivocationDifferentCreators,
    #[error("The sequence numbers in the equivocating votes are different.")]
    EquivocationDifferentSeqNumbers,
    #[error("The two votes are equal.")]
    EquivocationSameVote,
    #[error("The votes were created for a different instance ID.")]
    EquivocationInstanceId,
    #[error("The perpetrator is not a validator.")]
    UnknownPerpetrator,
    #[error("The signature is invalid.")]
    Signature,
}

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
    pub(crate) fn validate(
        &self,
        v_id: &C::ValidatorId,
        instance_id: &C::InstanceId,
    ) -> Result<(), EvidenceError> {
        match self {
            Evidence::Equivocation(vote1, vote2) => {
                if vote1.wire_vote.creator != vote2.wire_vote.creator {
                    return Err(EvidenceError::EquivocationDifferentCreators);
                }
                if vote1.wire_vote.seq_number != vote2.wire_vote.seq_number {
                    return Err(EvidenceError::EquivocationDifferentSeqNumbers);
                }
                if vote1.wire_vote.instance_id != *instance_id
                    || vote2.wire_vote.instance_id != *instance_id
                {
                    return Err(EvidenceError::EquivocationInstanceId);
                }
                if vote1 == vote2 {
                    return Err(EvidenceError::EquivocationSameVote);
                }
                if !C::verify_signature(&vote1.hash(), v_id, &vote1.signature)
                    || !C::verify_signature(&vote2.hash(), v_id, &vote2.signature)
                {
                    return Err(EvidenceError::Signature);
                }
                Ok(())
            }
        }
    }
}
