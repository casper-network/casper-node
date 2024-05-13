use core::{convert::TryFrom, num::TryFromIntError, time::Duration};
use std::collections::BTreeMap;

#[cfg(feature = "datasize")]
use datasize::DataSize;

use casper_types::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    EraId, ExecutionInfo, Key, PublicKey, TimeDiff, Timestamp, Transaction, ValidatorChange,
};

use super::GlobalStateQueryResult;

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

/// Type representing the reactor state name.
#[derive(Debug, PartialEq, Eq)]
pub struct ReactorStateName(String);

impl ReactorStateName {
    /// Constructs new reactor state name.
    pub fn new(value: impl ToString) -> Self {
        Self(value.to_string())
    }

    /// Retrieve the name as a `String`.
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl From<ReactorStateName> for String {
    fn from(reactor_state: ReactorStateName) -> Self {
        reactor_state.0
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

/// A transaction with execution info.
#[derive(Debug, PartialEq, Eq)]
pub struct TransactionWithExecutionInfo {
    transaction: Transaction,
    execution_info: Option<ExecutionInfo>,
}

impl TransactionWithExecutionInfo {
    /// Constructs new transaction with execution info.
    pub fn new(transaction: Transaction, execution_info: Option<ExecutionInfo>) -> Self {
        Self {
            transaction,
            execution_info,
        }
    }

    /// Converts `self` into the transaction and execution info.
    pub fn into_inner(self) -> (Transaction, Option<ExecutionInfo>) {
        (self.transaction, self.execution_info)
    }
}

impl ToBytes for TransactionWithExecutionInfo {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.transaction.serialized_length() + self.execution_info.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.transaction.write_bytes(writer)?;
        self.execution_info.write_bytes(writer)
    }
}

impl FromBytes for TransactionWithExecutionInfo {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (transaction, remainder) = FromBytes::from_bytes(bytes)?;
        let (execution_info, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((
            TransactionWithExecutionInfo::new(transaction, execution_info),
            remainder,
        ))
    }
}

/// A query result for a dictionary item, contains the dictionary item key and a global state query
/// result.
#[derive(Debug, Clone, PartialEq)]
pub struct DictionaryQueryResult {
    key: Key,
    query_result: GlobalStateQueryResult,
}

impl DictionaryQueryResult {
    /// Constructs new dictionary query result.
    pub fn new(key: Key, query_result: GlobalStateQueryResult) -> Self {
        Self { key, query_result }
    }

    /// Converts `self` into the dictionary item key and global state query result.
    pub fn into_inner(self) -> (Key, GlobalStateQueryResult) {
        (self.key, self.query_result)
    }
}

impl ToBytes for DictionaryQueryResult {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.key.write_bytes(writer)?;
        self.query_result.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.key.serialized_length() + self.query_result.serialized_length()
    }
}

impl FromBytes for DictionaryQueryResult {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (key, remainder) = FromBytes::from_bytes(bytes)?;
        let (query_result, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((DictionaryQueryResult::new(key, query_result), remainder))
    }
}

impl_bytesrepr_for_type_wrapper!(Uptime);
impl_bytesrepr_for_type_wrapper!(ConsensusValidatorChanges);
impl_bytesrepr_for_type_wrapper!(NetworkName);
impl_bytesrepr_for_type_wrapper!(ReactorStateName);
impl_bytesrepr_for_type_wrapper!(LastProgress);
impl_bytesrepr_for_type_wrapper!(GetTrieFullResult);

#[cfg(test)]
mod tests {
    use core::iter::FromIterator;
    use rand::Rng;

    use super::*;
    use casper_types::{
        execution::ExecutionResult, testing::TestRng, BlockHash, CLValue, StoredValue,
    };

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
    fn reactor_state_name_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&ReactorStateName::new(rng.random_string(5..20)));
    }

    #[test]
    fn last_progress_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&LastProgress::new(Timestamp::random(rng)));
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

    #[test]
    fn transaction_with_execution_info_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&TransactionWithExecutionInfo::new(
            Transaction::random(rng),
            rng.gen::<bool>().then(|| ExecutionInfo {
                block_hash: BlockHash::random(rng),
                block_height: rng.gen(),
                execution_result: rng.gen::<bool>().then(|| ExecutionResult::random(rng)),
            }),
        ));
    }

    #[test]
    fn dictionary_query_result_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&DictionaryQueryResult::new(
            Key::Account(rng.gen()),
            GlobalStateQueryResult::new(
                StoredValue::CLValue(CLValue::from_t(rng.gen::<i32>()).unwrap()),
                vec![],
            ),
        ));
    }
}
