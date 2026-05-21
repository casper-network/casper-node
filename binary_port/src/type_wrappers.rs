use core::{convert::TryFrom, num::TryFromIntError, time::Duration};
use std::collections::BTreeMap;

use casper_types::{
    bytesrepr::{self, Bytes, FromBytes, ToBytes},
    contracts::ContractHash,
    global_state::TrieMerkleProof,
    system::auction::DelegationRate,
    Account, AddressableEntity, BlockHash, ByteCode, Contract, ContractWasm, EntityAddr, EraId,
    ExecutionInfo, Key, PublicKey, StoredValue, TimeDiff, Timestamp, Transaction, ValidatorChange,
    U512,
};
use serde::Serialize;

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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
#[derive(Debug, PartialEq, Eq, Serialize)]
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
#[derive(Debug, PartialEq, Eq, Serialize)]
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
#[derive(Debug, PartialEq, Eq, Serialize)]
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
#[derive(Debug, PartialEq, Eq, Serialize)]
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

/// Type representing the reward of a validator or a delegator.
#[derive(Debug, PartialEq, Eq, Serialize)]
pub struct RewardResponse {
    amount: U512,
    era_id: EraId,
    delegation_rate: DelegationRate,
    switch_block_hash: BlockHash,
}

impl RewardResponse {
    /// Constructs new reward response.
    pub fn new(
        amount: U512,
        era_id: EraId,
        delegation_rate: DelegationRate,
        switch_block_hash: BlockHash,
    ) -> Self {
        Self {
            amount,
            era_id,
            delegation_rate,
            switch_block_hash,
        }
    }

    /// Returns the amount of the reward.
    pub fn amount(&self) -> U512 {
        self.amount
    }

    /// Returns the era ID.
    pub fn era_id(&self) -> EraId {
        self.era_id
    }

    /// Returns the delegation rate of the validator.
    pub fn delegation_rate(&self) -> DelegationRate {
        self.delegation_rate
    }

    /// Returns the switch block hash at which the reward was distributed.
    pub fn switch_block_hash(&self) -> BlockHash {
        self.switch_block_hash
    }
}

impl ToBytes for RewardResponse {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn serialized_length(&self) -> usize {
        self.amount.serialized_length()
            + self.era_id.serialized_length()
            + self.delegation_rate.serialized_length()
            + self.switch_block_hash.serialized_length()
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.amount.write_bytes(writer)?;
        self.era_id.write_bytes(writer)?;
        self.delegation_rate.write_bytes(writer)?;
        self.switch_block_hash.write_bytes(writer)
    }
}

impl FromBytes for RewardResponse {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (amount, remainder) = FromBytes::from_bytes(bytes)?;
        let (era_id, remainder) = FromBytes::from_bytes(remainder)?;
        let (delegation_rate, remainder) = FromBytes::from_bytes(remainder)?;
        let (switch_block_hash, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((
            RewardResponse::new(amount, era_id, delegation_rate, switch_block_hash),
            remainder,
        ))
    }
}

/// Describes the consensus status.
#[derive(Debug, PartialEq, Eq, Serialize)]
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
#[derive(Debug, PartialEq, Eq, Serialize)]
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

/// An account with its associated merkle proof.
#[derive(Debug, PartialEq)]
pub struct AccountInformation {
    account: Account,
    merkle_proof: Vec<TrieMerkleProof<Key, StoredValue>>,
}

impl AccountInformation {
    /// Constructs a new `AccountResponse`.
    pub fn new(account: Account, merkle_proof: Vec<TrieMerkleProof<Key, StoredValue>>) -> Self {
        Self {
            account,
            merkle_proof,
        }
    }

    /// Returns the inner `Account`.
    pub fn account(&self) -> &Account {
        &self.account
    }

    /// Returns the merkle proof.
    pub fn merkle_proof(&self) -> &Vec<TrieMerkleProof<Key, StoredValue>> {
        &self.merkle_proof
    }

    /// Converts `self` into the account and merkle proof.
    pub fn into_inner(self) -> (Account, Vec<TrieMerkleProof<Key, StoredValue>>) {
        (self.account, self.merkle_proof)
    }
}

impl ToBytes for AccountInformation {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.account.write_bytes(writer)?;
        self.merkle_proof.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.account.serialized_length() + self.merkle_proof.serialized_length()
    }
}

impl FromBytes for AccountInformation {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (account, remainder) = FromBytes::from_bytes(bytes)?;
        let (merkle_proof, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((AccountInformation::new(account, merkle_proof), remainder))
    }
}

/// A contract with its associated Wasm and merkle proof.
#[derive(Debug, PartialEq)]
pub struct ContractInformation {
    hash: ContractHash,
    contract: ValueWithProof<Contract>,
    wasm: Option<ValueWithProof<ContractWasm>>,
}

impl ContractInformation {
    /// Constructs new `ContractInformation`.
    pub fn new(
        hash: ContractHash,
        contract: ValueWithProof<Contract>,
        wasm: Option<ValueWithProof<ContractWasm>>,
    ) -> Self {
        Self {
            hash,
            contract,
            wasm,
        }
    }

    /// Returns the hash of the contract.
    pub fn hash(&self) -> ContractHash {
        self.hash
    }

    /// Returns the inner `Contract`.
    pub fn contract(&self) -> &Contract {
        &self.contract.value
    }

    /// Returns the Merkle proof of the contract.
    pub fn contract_proof(&self) -> &Vec<TrieMerkleProof<Key, StoredValue>> {
        &self.contract.merkle_proof
    }

    /// Returns the inner `ContractWasm` with its proof.
    pub fn wasm(&self) -> Option<&ValueWithProof<ContractWasm>> {
        self.wasm.as_ref()
    }

    /// Converts `self` into the contract hash, contract and Wasm.
    pub fn into_inner(
        self,
    ) -> (
        ContractHash,
        ValueWithProof<Contract>,
        Option<ValueWithProof<ContractWasm>>,
    ) {
        (self.hash, self.contract, self.wasm)
    }
}

impl ToBytes for ContractInformation {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.hash.write_bytes(writer)?;
        self.contract.write_bytes(writer)?;
        self.wasm.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.hash.serialized_length()
            + self.contract.serialized_length()
            + self.wasm.serialized_length()
    }
}

impl FromBytes for ContractInformation {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (hash, remainder) = FromBytes::from_bytes(bytes)?;
        let (contract, remainder) = FromBytes::from_bytes(remainder)?;
        let (wasm, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((ContractInformation::new(hash, contract, wasm), remainder))
    }
}

/// A contract entity with its associated ByteCode.
#[derive(Debug, PartialEq)]
pub struct AddressableEntityInformation {
    addr: EntityAddr,
    entity: ValueWithProof<AddressableEntity>,
    bytecode: Option<ValueWithProof<ByteCode>>,
}

impl AddressableEntityInformation {
    /// Constructs new contract entity with ByteCode.
    pub fn new(
        addr: EntityAddr,
        entity: ValueWithProof<AddressableEntity>,
        bytecode: Option<ValueWithProof<ByteCode>>,
    ) -> Self {
        Self {
            addr,
            entity,
            bytecode,
        }
    }

    /// Returns the entity address.
    pub fn addr(&self) -> EntityAddr {
        self.addr
    }

    /// Returns the inner `AddressableEntity`.
    pub fn entity(&self) -> &AddressableEntity {
        &self.entity.value
    }

    /// Returns the inner `ByteCodeWithProof`.
    pub fn entity_merkle_proof(&self) -> &Vec<TrieMerkleProof<Key, StoredValue>> {
        &self.entity.merkle_proof
    }

    /// Returns the inner `ByteCode`.
    pub fn bytecode(&self) -> Option<&ValueWithProof<ByteCode>> {
        self.bytecode.as_ref()
    }

    /// Converts `self` into the entity address, entity and ByteCode.
    pub fn into_inner(
        self,
    ) -> (
        EntityAddr,
        ValueWithProof<AddressableEntity>,
        Option<ValueWithProof<ByteCode>>,
    ) {
        (self.addr, self.entity, self.bytecode)
    }
}

impl ToBytes for AddressableEntityInformation {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.addr.write_bytes(writer)?;
        self.entity.write_bytes(writer)?;
        self.bytecode.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.addr.serialized_length()
            + self.entity.serialized_length()
            + self.bytecode.serialized_length()
    }
}

impl FromBytes for AddressableEntityInformation {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (addr, remainder) = FromBytes::from_bytes(bytes)?;
        let (entity, remainder) = FromBytes::from_bytes(remainder)?;
        let (bytecode, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((
            AddressableEntityInformation::new(addr, entity, bytecode),
            remainder,
        ))
    }
}

/// A value with its associated Merkle proof.
#[derive(Debug, PartialEq)]
pub struct ValueWithProof<T> {
    value: T,
    merkle_proof: Vec<TrieMerkleProof<Key, StoredValue>>,
}

impl<T> ValueWithProof<T> {
    /// Constructs a new `ValueWithProof`.
    pub fn new(value: T, merkle_proof: Vec<TrieMerkleProof<Key, StoredValue>>) -> Self {
        Self {
            value,
            merkle_proof,
        }
    }

    /// Returns the value.
    pub fn value(&self) -> &T {
        &self.value
    }

    /// Returns the Merkle proof.
    pub fn merkle_proof(&self) -> &[TrieMerkleProof<Key, StoredValue>] {
        &self.merkle_proof
    }

    /// Converts `self` into the value and Merkle proof.
    pub fn into_inner(self) -> (T, Vec<TrieMerkleProof<Key, StoredValue>>) {
        (self.value, self.merkle_proof)
    }
}

impl<T: ToBytes> ToBytes for ValueWithProof<T> {
    fn to_bytes(&self) -> Result<Vec<u8>, bytesrepr::Error> {
        let mut buffer = bytesrepr::allocate_buffer(self)?;
        self.write_bytes(&mut buffer)?;
        Ok(buffer)
    }

    fn write_bytes(&self, writer: &mut Vec<u8>) -> Result<(), bytesrepr::Error> {
        self.value.write_bytes(writer)?;
        self.merkle_proof.write_bytes(writer)
    }

    fn serialized_length(&self) -> usize {
        self.value.serialized_length() + self.merkle_proof.serialized_length()
    }
}

impl<T: FromBytes> FromBytes for ValueWithProof<T> {
    fn from_bytes(bytes: &[u8]) -> Result<(Self, &[u8]), bytesrepr::Error> {
        let (value, remainder) = FromBytes::from_bytes(bytes)?;
        let (merkle_proof, remainder) = FromBytes::from_bytes(remainder)?;
        Ok((ValueWithProof::new(value, merkle_proof), remainder))
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
        contracts::ContractPackageHash, execution::ExecutionResult, testing::TestRng, BlockHash,
        CLValue, ContractWasmHash, StoredValue,
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
    fn reward_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&RewardResponse::new(
            rng.gen(),
            EraId::random(rng),
            rng.gen(),
            BlockHash::random(rng),
        ));
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

    #[test]
    fn contract_with_wasm_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&ContractInformation::new(
            ContractHash::new(rng.gen()),
            ValueWithProof::new(
                Contract::new(
                    ContractPackageHash::new(rng.gen()),
                    ContractWasmHash::new(rng.gen()),
                    Default::default(),
                    Default::default(),
                    Default::default(),
                ),
                Default::default(),
            ),
            rng.gen::<bool>().then(|| {
                ValueWithProof::new(
                    ContractWasm::new(rng.random_vec(10..50)),
                    Default::default(),
                )
            }),
        ));
    }

    #[test]
    fn addressable_entity_with_byte_code_roundtrip() {
        let rng = &mut TestRng::new();
        bytesrepr::test_serialization_roundtrip(&AddressableEntityInformation::new(
            rng.gen(),
            ValueWithProof::new(AddressableEntity::example().clone(), Default::default()),
            rng.gen::<bool>().then(|| {
                ValueWithProof::new(
                    ByteCode::new(rng.gen(), rng.random_vec(10..50)),
                    Default::default(),
                )
            }),
        ));
    }
}
