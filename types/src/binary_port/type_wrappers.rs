//! Type aliases.

use core::{convert::TryFrom, num::TryFromIntError, time::Duration};

use alloc::{
    collections::BTreeMap,
    string::{String, ToString},
    vec::Vec,
};
#[cfg(feature = "datasize")]
use datasize::DataSize;

use crate::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    contract_messages::Messages,
    execution::ExecutionResultV2,
    EraId, PublicKey, TimeDiff, Timestamp, ValidatorChange,
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

            fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
                self.0.write_bytes(writer)
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Uptime(u64);

impl Uptime {
    /// Constructs new uptime.
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    /// Retrieve the inner value.
    pub fn into_inner(self) -> u64 {
        self.0
    }
}

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
#[derive(Debug, PartialEq, Eq)]
#[cfg_attr(feature = "datasize", derive(DataSize))]
pub struct ConsensusValidatorChanges(BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>>);

impl ConsensusValidatorChanges {
    /// Constructs new consensus validator changes.
    pub fn new(value: BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>>) -> Self {
        Self(value)
    }

    /// Retrieve the inner value.
    pub fn into_inner(self) -> BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>> {
        self.0
    }
}

impl From<ConsensusValidatorChanges> for BTreeMap<PublicKey, Vec<(EraId, ValidatorChange)>> {
    fn from(consensus_validator_changes: ConsensusValidatorChanges) -> Self {
        consensus_validator_changes.0
    }
}

/// Type representing network name.
#[derive(Debug, PartialEq, Eq)]
pub struct NetworkName(String);

impl NetworkName {
    /// Constructs new network name.
    pub fn new(value: impl ToString) -> Self {
        Self(value.to_string())
    }

    /// Retrieve the inner value.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl From<NetworkName> for String {
    fn from(network_name: NetworkName) -> Self {
        network_name.0
    }
}

/// Type representing last progress of the sync process.
#[derive(Debug, PartialEq, Eq)]
pub struct LastProgress(Timestamp);

impl LastProgress {
    /// Constructs new last progress.
    pub fn new(value: Timestamp) -> Self {
        Self(value)
    }

    /// Retrieve the inner value.
    pub fn into_inner(self) -> Timestamp {
        self.0
    }
}

impl From<LastProgress> for Timestamp {
    fn from(last_progress: LastProgress) -> Self {
        last_progress.0
    }
}

/// Type representing results of checking whether a block is in the highest sequence of available
/// blocks.
#[derive(Debug, PartialEq, Eq)]
pub struct HighestBlockSequenceCheckResult(bool);

impl HighestBlockSequenceCheckResult {
    /// Constructs new highest block sequence check result.
    pub fn new(value: bool) -> Self {
        Self(value)
    }

    /// Returns the inner value.
    pub fn into_inner(self) -> bool {
        self.0
    }
}

impl From<HighestBlockSequenceCheckResult> for bool {
    fn from(highest_block_sequence_check_result: HighestBlockSequenceCheckResult) -> Self {
        highest_block_sequence_check_result.0
    }
}

/// Type representing results of the speculative execution.
#[derive(Debug, PartialEq, Eq)]
pub struct SpeculativeExecutionResult(Option<(ExecutionResultV2, Messages)>);

impl SpeculativeExecutionResult {
    /// Constructs new speculative execution result.
    pub fn new(value: Option<(ExecutionResultV2, Messages)>) -> Self {
        Self(value)
    }

    /// Returns the inner value.
    pub fn into_inner(self) -> Option<(ExecutionResultV2, Messages)> {
        self.0
    }
}

/// Type representing results of the get full trie request.
#[derive(Debug, PartialEq, Eq)]
pub struct GetTrieFullResult(Option<Bytes>);

impl GetTrieFullResult {
    /// Constructs new get trie result.
    pub fn new(value: Option<Bytes>) -> Self {
        Self(value)
    }

    /// Returns the inner value.
    pub fn into_inner(self) -> Option<Bytes> {
        self.0
    }
}

/// Describes the consensus status.
#[derive(Debug, PartialEq, Eq)]
pub struct ConsensusStatus {
    validator_public_key: PublicKey,
    round_length: Option<TimeDiff>,
}

impl ConsensusStatus {
    /// Constructs new consensus status.
    pub fn new(validator_public_key: PublicKey, round_length: Option<TimeDiff>) -> Self {
        Self {
            validator_public_key,
            round_length,
        }
    }

    /// Returns the validator public key.
    pub fn validator_public_key(&self) -> &PublicKey {
        &self.validator_public_key
    }

    /// Returns the round length.
    pub fn round_length(&self) -> Option<TimeDiff> {
        self.round_length
    }
}

impl ToBytes for ConsensusStatus {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.validator_public_key.serialized_length() + self.round_length.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.validator_public_key.write_bytes(writer)?;
        self.round_length.write_bytes(writer)
    }
}

impl FromBytes for ConsensusStatus {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (validator_public_key, remainder) = FromBytes::from_bytes(bytes)?;
        let (round_length, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((
            ConsensusStatus::new(validator_public_key, round_length),
            remainder,
        ))
    }
}

impl_bytesrepr_for_type_wrapper!(Uptime);
impl_bytesrepr_for_type_wrapper!(ConsensusValidatorChanges);
impl_bytesrepr_for_type_wrapper!(NetworkName);
impl_bytesrepr_for_type_wrapper!(LastProgress);
impl_bytesrepr_for_type_wrapper!(HighestBlockSequenceCheckResult);
impl_bytesrepr_for_type_wrapper!(SpeculativeExecutionResult);
impl_bytesrepr_for_type_wrapper!(GetTrieFullResult);

#[cfg(test)]
mod tests {
    use core::iter::FromIterator;
    use rand::Rng;

    use super::*;
    use crate::testing::TestRng;

    #[test]
    fn uptime_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&Uptime::new(rng.gen()));
    }

    #[test]
    fn consensus_validator_changes_roundtrip() {
        let rng = &mut TestRng::new();
        let map = BTreeMap::from_iter([(
            PublicKey::random(rng),
            vec![(EraId::random(rng), ValidatorChange::random(rng))],
        )]);
        bytesrepr::test_serialization_roundtrip(&ConsensusValidatorChanges::new(map));
    }

    #[test]
    fn network_name_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&NetworkName::new(rng.random_string(5..20)));
    }

    #[test]
    fn last_progress_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&LastProgress::new(Timestamp::random(rng)));
    }

    #[test]
    fn highest_block_sequence_check_result_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&HighestBlockSequenceCheckResult::new(rng.gen()));
    }

    #[test]
    fn speculative_execution_result_roundtrip() {
        let rng = &mut TestRng::new();
        if rng.gen_bool(0.5) {
            bytesrepr::test_serialization_roundtrip(&SpeculativeExecutionResult::new(None));
        } else {
            bytesrepr::test_serialization_roundtrip(&SpeculativeExecutionResult::new(Some((
                ExecutionResultV2::random(rng),
                rng.random_vec(0..20),
            ))));
        }
    }

    #[test]
    fn get_trie_full_result_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&GetTrieFullResult::new(rng.gen()));
    }

    #[test]
    fn consensus_status_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&ConsensusStatus::new(
            PublicKey::random(rng),
            Some(TimeDiff::from_millis(rng.gen())),
        ));
    }
}
