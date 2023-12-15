//! Type aliases.

use core::{convert::TryFrom, num::TryFromIntError, time::Duration};

use alloc::{collections::BTreeMap, string::String, vec::Vec};
#[cfg(feature = "datasize")]
use datasize::DataSize;

use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    contract_messages::Messages,
    execution::ExecutionResultV2,
    EraId, PublicKey, StoredValue, TimeDiff, Timestamp, ValidatorChange,
};

// `bytesrepr` implementations for type wrappers are repetitive, hence this macro helper. We should
// get rid of this after we introduce the proper "bytesrepr-derive" proc macro.
macro_rules! impl_bytesrepr_for_type_wrapper {
    ($t:ident) => {
        impl ToBytes for $t {
            fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
                self.0.to_bytes()
            }

            fn serialized_length(&self) -> usize {
                self.0.serialized_length()
            }
        }

        impl FromBytes for $t {
            fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
                let (inner, remainder) = FromBytes::from_bytes(bytes)?;
                Ok(($t(inner), remainder))
            }
        }
    };
}

/// Type representing uptime.
#[derive(Debug, Clone, Copy)]
pub struct Uptime(pub u64);

impl From<Uptime> for Duration {
    fn from(uptime: Uptime) -> Self {
        Duration::from_secs(uptime.0)
    }
}

impl TryFrom<Uptime> for TimeDiff {
    type Error = TryFromIntError;

    fn try_from(uptime: Uptime) -> Result<Self, Self::Error> {
        u32::try_from(uptime.0).map(TimeDiff::from_seconds)
    }
}

/// Type representing changes in consensus validators.
#[derive(Debug)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct ConsensusValidatorChanges(pub BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>>);

impl From<ConsensusValidatorChanges> for BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>> {
    fn from(consensus_validator_changes: ConsensusValidatorChanges) -> Self {
        consensus_validator_changes.0
    }
}

/// Type representing network name.
#[derive(Debug)]
pub struct NetworkName(pub String);

impl From<NetworkName> for String {
    fn from(network_name: NetworkName) -> Self {
        network_name.0
    }
}

/// Type representing last progress of the sync process.
#[derive(Debug)]
pub struct LastProgress(pub Timestamp);

impl From<LastProgress> for Timestamp {
    fn from(last_progress: LastProgress) -> Self {
        last_progress.0
    }
}

/// Type representing results of checking whether a block is in the highest sequence of available
/// blocks.
#[derive(Debug)]
pub struct HighestBlockSequenceCheckResult(pub bool);

impl From<HighestBlockSequenceCheckResult> for bool {
    fn from(highest_block_sequence_check_result: HighestBlockSequenceCheckResult) -> Self {
        highest_block_sequence_check_result.0
    }
}

/// Type representing results of the speculative execution.
#[derive(Debug)]
pub struct SpeculativeExecutionResult(pub Option<(ExecutionResultV2, Messages)>);

impl SpeculativeExecutionResult {
    /// Returns the inner value.
    pub fn into_inner(self) -> Option<(ExecutionResultV2, Messages)> {
        self.0
    }
}

/// Type representing results of the get full trie request.
#[derive(Debug)]
pub struct GetTrieFullResult(pub Option<Bytes>);

impl GetTrieFullResult {
    /// Returns the inner value.
    pub fn into_inner(self) -> Option<Bytes> {
        self.0
    }
}

/// Type representing successful result of GetAllValues request.
#[derive(Debug, PartialEq)]
pub struct StoredValues(pub Vec<StoredValue>);

impl StoredValues {
    /// Returns the inner value.
    pub fn into_inner(self) -> Vec<StoredValue> {
        self.0
    }
}

/// Describes the consensus status.
#[derive(Debug, PartialEq)]
pub struct ConsensusStatus(pub (PublicKey, Option<TimeDiff>));

impl ConsensusStatus {
    /// Returns the inner value.
    pub fn into_inner(self) -> (PublicKey, Option<TimeDiff>) {
        self.0
    }
}

/// Type representing bytes from the state store db.
#[derive(Debug, PartialEq)]
pub struct StateStoreValue(pub Vec<u8>);

impl StateStoreValue {
    /// Returns the inner value.
    pub fn into_inner(self) -> Vec<u8> {
        self.0
    }
}

impl_bytesrepr_for_type_wrapper!(Uptime);
impl_bytesrepr_for_type_wrapper!(ConsensusValidatorChanges);
impl_bytesrepr_for_type_wrapper!(NetworkName);
impl_bytesrepr_for_type_wrapper!(LastProgress);
impl_bytesrepr_for_type_wrapper!(HighestBlockSequenceCheckResult);
impl_bytesrepr_for_type_wrapper!(SpeculativeExecutionResult);
impl_bytesrepr_for_type_wrapper!(GetTrieFullResult);
impl_bytesrepr_for_type_wrapper!(StoredValues);
impl_bytesrepr_for_type_wrapper!(ConsensusStatus);
impl_bytesrepr_for_type_wrapper!(StateStoreValue);
