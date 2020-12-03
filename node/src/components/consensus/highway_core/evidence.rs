use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::validators::ValidatorIndex;
use crate::components::consensus::{highway_core::highway::SignedWireUnit, traits::Context};

/// An error due to invalid evidence.
#[derive(Debug, Error, PartialEq)]
pub(crate) enum EvidenceError {
    #[error("The creators in the equivocating units are different.")]
    EquivocationDifferentCreators,
    #[error("The sequence numbers in the equivocating units are different.")]
    EquivocationDifferentSeqNumbers,
    #[error("The two units are equal.")]
    EquivocationSameUnit,
    #[error("The units were created for a different instance ID.")]
    EquivocationInstanceId,
    #[error("The perpetrator is not a validator.")]
    UnknownPerpetrator,
    #[error("The signature is invalid.")]
    Signature,
}

/// Evidence that a validator is faulty.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize, Hash)]
#[serde(bound(
    serialize = "C::Hash: Serialize",
    deserialize = "C::Hash: Deserialize<'de>",
))]
pub(crate) enum Evidence<C: Context> {
    /// The validator produced two units with the same sequence number.
    Equivocation(SignedWireUnit<C>, SignedWireUnit<C>),
}

impl<C: Context> Evidence<C> {
    // TODO: Verify whether the evidence is conclusive. Or as part of deserialization?

    /// Returns the ID of the faulty validator.
    pub(crate) fn perpetrator(&self) -> ValidatorIndex {
        match self {
            Evidence::Equivocation(unit0, _) => unit0.wire_unit.creator,
        }
    }

    /// Validates the evidence and returns `Ok(())` if it is valid.
    /// "Validation" can mean different things for different type of evidence.
    ///
    /// - For an equivocation, it checks whether the creators, sequence numbers and instance IDs of
    /// the two units are the same.
    pub(crate) fn validate(
        &self,
        v_id: &C::ValidatorId,
        instance_id: &C::InstanceId,
    ) -> Result<(), EvidenceError> {
        match self {
            Evidence::Equivocation(unit1, unit2) => {
                if unit1.wire_unit.creator != unit2.wire_unit.creator {
                    return Err(EvidenceError::EquivocationDifferentCreators);
                }
                if unit1.wire_unit.seq_number != unit2.wire_unit.seq_number {
                    return Err(EvidenceError::EquivocationDifferentSeqNumbers);
                }
                if unit1.wire_unit.instance_id != *instance_id
                    || unit2.wire_unit.instance_id != *instance_id
                {
                    return Err(EvidenceError::EquivocationInstanceId);
                }
                if unit1 == unit2 {
                    return Err(EvidenceError::EquivocationSameUnit);
                }
                if !C::verify_signature(&unit1.hash(), v_id, &unit1.signature)
                    || !C::verify_signature(&unit2.hash(), v_id, &unit2.signature)
                {
                    return Err(EvidenceError::Signature);
                }
                Ok(())
            }
        }
    }
}
