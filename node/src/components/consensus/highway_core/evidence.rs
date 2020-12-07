use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::validators::ValidatorIndex;
use crate::{
    components::consensus::{highway_core::highway::SignedWireUnit, traits::Context},
    types::Timestamp,
};

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
pub enum Evidence<C: Context> {
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

    /// Returns the instance ID within the evidence.
    pub(crate) fn instance(&self) -> C::InstanceId {
        match self {
            Evidence::Equivocation(unit0, _) => unit0.wire_unit.instance_id,
        }
    }

    /// Returns the signature within the unit.
    pub(crate) fn signature(&self) -> (C::Signature, C::Signature) {
        match self {
            Evidence::Equivocation(unit0, unit1) => (unit0.signature, unit1.signature),
        }
    }

    /// Returns the timestamps of the each unit
    pub(crate) fn timestamps(&self) -> (Timestamp, Timestamp) {
        match self {
            Evidence::Equivocation(unit0, unit1) => {
                (unit0.wire_unit.timestamp, unit1.wire_unit.timestamp)
            }
        }
    }

    /// Returns the sequence number
    pub(crate) fn sequence_number(&self) -> u64 {
        match self {
            Evidence::Equivocation(unit0, _) => unit0.wire_unit.seq_number,
        }
    }

    /// Returns the round exponents of the units
    pub(crate) fn round_exps(&self) -> (u8, u8) {
        match self {
            Evidence::Equivocation(unit0, unit1) => {
                (unit0.wire_unit.round_exp, unit1.wire_unit.round_exp)
            }
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
