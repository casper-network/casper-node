use std::path::PathBuf;

use alloy_consensus::{crypto::secp256k1, SignableTransaction, TxEip7702, TxEnvelope, TxLegacy};
use alloy_eips::{
    eip2718::Encodable2718,
    eip7702::{
        Authorization as AlloyAuthorization, SignedAuthorization as AlloySignedAuthorization,
    },
};
use alloy_primitives::{Address as AlloyAddress, Signature, TxKind, B256, U256};
use casper_executor_evm::{
    BlockContext, BlockHashProvider, BlockHashProviderResult, CallRequest, CallValidation, Error,
    EvmExecutor, ExecuteKind, ExecuteRequest, ExecutionStatus, EMPTY_CODE_HASH,
};
use casper_storage::{
    data_access_layer::{GenesisRequest, GenesisResult},
    global_state::{
        self,
        error::Error as GlobalStateError,
        state::{lmdb::LmdbGlobalStateView, CommitProvider, StateProvider, StateReader},
    },
    TrackingCopy,
};
use casper_types::{
    bytesrepr::{FromBytes, ToBytes},
    contracts::NamedKeys,
    evm, AccessRights, Account, BlockHash, CLValue, ChainspecRegistry, Digest, GenesisAccount,
    GenesisConfig, HoldBalanceHandling, Key, Motes, ProtocolVersion, PublicKey, SecretKey,
    StorageCosts, StoredValue, SystemConfig, Timestamp, URef, WasmConfig, U256 as CasperU256, U512,
};
use revm::bytecode::opcode;

const SIGNING_SECRET: [u8; 32] = [7; 32];
const AUTHORIZATION_SECRET: [u8; 32] = [8; 32];

fn tracking_copy() -> (TrackingCopy<LmdbGlobalStateView>, impl Send) {
    let accounts = (1u8..=3)
        .map(|seed| {
            let secret_key =
                SecretKey::ed25519_from_bytes([seed; SecretKey::ED25519_LENGTH]).unwrap();
            GenesisAccount::Account {
                public_key: PublicKey::from(&secret_key),
                balance: Motes::new(U512::from(1_000_000_000_000u64)),
                validator: None,
            }
        })
        .collect();

    let (global_state, _state_root_hash, tempdir) =
        global_state::state::lmdb::make_temporary_global_state([]);
    let genesis_config = GenesisConfig::new(
        accounts,
        WasmConfig::default(),
        SystemConfig::default(),
        10,
        10,
        0,
        Default::default(),
        14,
        Timestamp::now().millis(),
        HoldBalanceHandling::Accrued,
        0,
        true,
        None,
        StorageCosts::default(),
        0,
    );
    let genesis_request = GenesisRequest::new(
        Digest::hash("evm-executor-test-genesis"),
        ProtocolVersion::V2_0_0,
        genesis_config,
        ChainspecRegistry::new_with_genesis(b"", b""),
    );
    let post_state_hash = match global_state.genesis(genesis_request) {
        GenesisResult::Failure(failure) => panic!("failed to run genesis: {failure:?}"),
        GenesisResult::Fatal(fatal) => panic!("fatal error while running genesis: {fatal}"),
        GenesisResult::Success {
            post_state_hash, ..
        } => post_state_hash,
    };
    let reader = global_state
        .checkout(post_state_hash)
        .expect("checkout should not fail")
        .expect("post-genesis root should exist");
    (TrackingCopy::new(reader, 5, false), tempdir)
}

fn executor(spec: evm::EvmSpec) -> EvmExecutor {
    EvmExecutor::new(evm::EvmConfig {
        enabled: true,
        chain_id: 7,
        spec,
        block_gas_limit: 30_000_000,
        base_fee: 0,
    })
}

fn block() -> BlockContext {
    BlockContext {
        number: 1,
        timestamp: 1_714_000_000,
        beneficiary: evm::Address::ZERO,
        gas_limit: None,
        base_fee: None,
    }
}

#[derive(Clone, Copy)]
struct HeightBlockHashProvider;

impl BlockHashProvider for HeightBlockHashProvider {
    fn block_hash(&self, block_height: u64) -> BlockHashProviderResult<Option<BlockHash>> {
        Ok(Some(block_hash_for_height(block_height)))
    }
}

fn block_hash_for_height(block_height: u64) -> BlockHash {
    let mut bytes = [0u8; BlockHash::LENGTH];
    bytes[24..].copy_from_slice(&block_height.to_be_bytes());
    BlockHash::new(Digest::from_raw(bytes))
}

fn init_code_returning(runtime: Vec<u8>) -> Vec<u8> {
    let runtime_len = u8::try_from(runtime.len()).expect("runtime should fit in PUSH1");
    let runtime_offset = 12u8;
    let mut init_code = vec![
        // memory[0..runtime_len] = code[runtime_offset..runtime_offset + runtime_len]
        opcode::PUSH1,
        runtime_len,
        opcode::PUSH1,
        runtime_offset,
        opcode::PUSH1,
        0,
        opcode::CODECOPY,
        // return memory[0..runtime_len]
        opcode::PUSH1,
        runtime_len,
        opcode::PUSH1,
        0,
        opcode::RETURN,
    ];
    assert_eq!(init_code.len(), usize::from(runtime_offset));
    init_code.extend(runtime);
    init_code
}

fn blockhash_contract_init_code() -> Vec<u8> {
    let runtime = vec![
        // bytes32 hash = blockhash(1);
        opcode::PUSH1,
        1,
        opcode::BLOCKHASH,
        // mstore(0, hash);
        opcode::PUSH1,
        0,
        opcode::MSTORE,
        // return abi.encode(hash);
        opcode::PUSH1,
        32,
        opcode::PUSH1,
        0,
        opcode::RETURN,
    ];
    init_code_returning(runtime)
}

fn return_word_contract_init_code(value: u8) -> Vec<u8> {
    let runtime = vec![
        opcode::PUSH1,
        value,
        opcode::PUSH1,
        0,
        opcode::MSTORE,
        opcode::PUSH1,
        32,
        opcode::PUSH1,
        0,
        opcode::RETURN,
    ];
    init_code_returning(runtime)
}

fn reverting_contract_init_code() -> Vec<u8> {
    let runtime = vec![opcode::PUSH1, 0, opcode::PUSH1, 0, opcode::REVERT];
    init_code_returning(runtime)
}

fn call_request(
    from: evm::Address,
    to: Option<evm::Address>,
    input: Vec<u8>,
    value: CasperU256,
) -> ExecuteRequest {
    ExecuteRequest {
        block: block(),
        kind: ExecuteKind::Call(CallRequest {
            from,
            to,
            value,
            input,
            gas_limit: 5_000_000,
            gas_price: 0,
            nonce: 0,
            validation: CallValidation::UncheckedSimulation,
        }),
    }
}

fn checked_call_request(
    from: evm::Address,
    to: Option<evm::Address>,
    input: Vec<u8>,
    value: CasperU256,
) -> ExecuteRequest {
    ExecuteRequest {
        block: block(),
        kind: ExecuteKind::Call(CallRequest {
            from,
            to,
            value,
            input,
            gas_limit: 5_000_000,
            gas_price: 0,
            nonce: 0,
            validation: CallValidation::Checked,
        }),
    }
}

fn execute_call<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    executor: &EvmExecutor,
    tracking_copy: &mut TrackingCopy<R>,
    from: evm::Address,
    to: Option<evm::Address>,
    input: Vec<u8>,
) -> casper_executor_evm::ExecutionOutcome {
    let outcome = executor
        .execute(
            tracking_copy,
            call_request(from, to, input, CasperU256::zero()),
        )
        .expect("EVM execution should succeed");
    assert_eq!(outcome.status, ExecutionStatus::Success);
    outcome
}

fn execute_transaction<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    executor: &EvmExecutor,
    tracking_copy: &mut TrackingCopy<R>,
    transaction: evm::Transaction,
) -> casper_executor_evm::ExecutionOutcome {
    executor
        .execute(
            tracking_copy,
            ExecuteRequest {
                block: block(),
                kind: ExecuteKind::Transaction(transaction),
            },
        )
        .expect("EVM transaction execution should succeed")
}

fn deploy_code<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    executor: &EvmExecutor,
    tracking_copy: &mut TrackingCopy<R>,
    from: evm::Address,
    code: Vec<u8>,
) -> evm::Address {
    execute_call(executor, tracking_copy, from, None, code)
        .created_contract_address
        .expect("deploy should return a contract address")
}

fn deploy<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    executor: &EvmExecutor,
    tracking_copy: &mut TrackingCopy<R>,
    from: evm::Address,
    name: &str,
) -> evm::Address {
    execute_call(executor, tracking_copy, from, None, contract_bin(name))
        .created_contract_address
        .expect("deploy should return a contract address")
}

fn contract_bin(name: &str) -> Vec<u8> {
    let path = artifact_path(format!("{name}.bin"));
    let hex = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()));
    decode_hex(hex.trim())
}

fn artifact_path(file_name: String) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("target")
        .join("evm-contracts")
        .join(file_name)
}

fn selector(signature: &str) -> Vec<u8> {
    revm::primitives::keccak256(signature.as_bytes())[..4].to_vec()
}

type AbiWord = [u8; evm::HASH_LENGTH];

fn calldata(signature: &str, args: &[AbiWord]) -> Vec<u8> {
    let mut bytes = selector(signature);
    for arg in args {
        bytes.extend_from_slice(arg);
    }
    bytes
}

fn word(value: u64) -> AbiWord {
    let mut bytes = [0u8; 32];
    bytes[24..].copy_from_slice(&value.to_be_bytes());
    bytes
}

fn storage_word(value: u64) -> CasperU256 {
    CasperU256::from(value)
}

fn address_word(address: evm::Address) -> AbiWord {
    let mut bytes = [0u8; 32];
    bytes[12..].copy_from_slice(address.as_bytes());
    bytes
}

fn decode_word(output: &[u8]) -> u64 {
    assert_eq!(output.len(), 32);
    u64::from_be_bytes(output[24..].try_into().unwrap())
}

fn decode_address(output: &[u8]) -> evm::Address {
    assert_eq!(output.len(), 32);
    let mut bytes = [0u8; 20];
    bytes.copy_from_slice(&output[12..]);
    evm::Address::new(bytes)
}

fn decode_hex(hex: &str) -> Vec<u8> {
    assert!(hex.len() % 2 == 0, "hex input must have an even length");
    (0..hex.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&hex[index..index + 2], 16).unwrap())
        .collect()
}

fn to_alloy_address(address: evm::Address) -> AlloyAddress {
    AlloyAddress::from(address.value())
}

fn alloy_address_to_evm(address: AlloyAddress) -> evm::Address {
    evm::Address::new(address.into_array())
}

fn legacy_transaction(chain_id: Option<u64>) -> evm::Transaction {
    let tx = TxLegacy {
        chain_id,
        nonce: 0,
        gas_price: 1,
        gas_limit: 21_000,
        to: TxKind::Call(AlloyAddress::from([1u8; 20])),
        value: U256::ZERO,
        input: Default::default(),
    };
    let tx = tx.into_signed(Signature::test_signature().with_parity(true));
    let envelope: TxEnvelope = tx.into();
    evm::Transaction::from_signed_rlp(
        envelope.encoded_2718(),
        Timestamp::zero(),
        casper_types::TimeDiff::from_seconds(60),
    )
    .expect("transaction should decode")
}

fn eip7702_transaction(
    to: evm::Address,
    delegate: evm::Address,
    authorization_nonce: u64,
    transaction_nonce: u64,
    input: Vec<u8>,
) -> (evm::Transaction, evm::Address) {
    let authorization = signed_authorization(delegate, authorization_nonce);
    let authority = alloy_address_to_evm(
        authorization
            .recover_authority()
            .expect("authorization should recover authority"),
    );
    let tx = TxEip7702 {
        chain_id: 7,
        nonce: transaction_nonce,
        gas_limit: 1_000_000,
        max_fee_per_gas: 1,
        max_priority_fee_per_gas: 0,
        to: to_alloy_address(to),
        value: U256::ZERO,
        access_list: Default::default(),
        authorization_list: vec![authorization],
        input: input.into(),
    };
    let signature = secp256k1::sign_message(B256::from(SIGNING_SECRET), tx.signature_hash())
        .expect("transaction signing should succeed");
    let envelope: TxEnvelope = tx.into_signed(signature).into();
    let transaction = evm::Transaction::from_signed_rlp(
        envelope.encoded_2718(),
        Timestamp::zero(),
        casper_types::TimeDiff::from_seconds(60),
    )
    .expect("transaction should decode");
    (transaction, authority)
}

fn eip7702_transaction_without_priority_fee(transaction: evm::Transaction) -> evm::Transaction {
    assert_eq!(transaction.max_priority_fee_per_gas(), Some(0));
    let mut bytes = transaction
        .to_bytes()
        .expect("transaction should serialize");
    let mut offset = 0;
    offset += transaction.timestamp().serialized_length();
    offset += transaction.ttl().serialized_length();
    offset += transaction.hash().serialized_length();
    offset += transaction.from().serialized_length();
    offset += transaction.kind().serialized_length();
    offset += transaction.to().serialized_length();
    offset += transaction.nonce().serialized_length();
    offset += transaction.gas_limit().serialized_length();
    offset += transaction.gas_price().serialized_length();
    offset += transaction.max_fee_per_gas().serialized_length();

    assert_eq!(bytes[offset], 1);
    let some_priority_length = transaction.max_priority_fee_per_gas().serialized_length();
    let none_priority = Option::<u128>::None
        .to_bytes()
        .expect("none priority fee should serialize");
    bytes.splice(offset..offset + some_priority_length, none_priority);

    let (transaction, remainder) =
        evm::Transaction::from_bytes(&bytes).expect("transaction should deserialize");
    assert!(remainder.is_empty());
    assert_eq!(transaction.max_priority_fee_per_gas(), None);
    transaction
}

fn signed_authorization(delegate: evm::Address, nonce: u64) -> AlloySignedAuthorization {
    let authorization = AlloyAuthorization {
        chain_id: U256::from(7),
        address: to_alloy_address(delegate),
        nonce,
    };
    let signature = secp256k1::sign_message(
        B256::from(AUTHORIZATION_SECRET),
        authorization.signature_hash(),
    )
    .expect("authorization signing should succeed");
    authorization.into_signed(signature)
}

fn authorization_authority() -> evm::Address {
    alloy_address_to_evm(
        signed_authorization(evm::Address::ZERO, 0)
            .recover_authority()
            .expect("authorization should recover authority"),
    )
}

fn legacy_transaction_without_chain_id() -> evm::Transaction {
    legacy_transaction(None)
}

fn read_storage<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
    slot: CasperU256,
) -> Option<CasperU256> {
    match tracking_copy
        .read(&Key::Evm(evm::EvmAddr::Storage(evm::StorageAddr::new(
            address, slot,
        ))))
        .expect("storage read should not fail")
    {
        Some(StoredValue::CLValue(value)) => value.into_t::<CasperU256>().ok(),
        Some(other) => panic!("unexpected storage value: {other:?}"),
        None => None,
    }
}

fn read_balance<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
) -> U512 {
    let purse = match tracking_copy
        .read(&Key::Evm(evm::EvmAddr::Account(address)))
        .expect("account read should not fail")
    {
        Some(StoredValue::CLValue(value)) => match value.into_t::<Key>().unwrap() {
            Key::URef(uref) => uref,
            Key::Account(account_hash) => match tracking_copy
                .read(&Key::Account(account_hash))
                .expect("linked account read should not fail")
            {
                Some(StoredValue::Account(account)) => account.main_purse(),
                Some(other) => panic!("unexpected linked account value: {other:?}"),
                None => return U512::zero(),
            },
            other => panic!("unexpected EVM account identity key: {other:?}"),
        },
        Some(other) => panic!("unexpected account value: {other:?}"),
        None => return U512::zero(),
    };
    match tracking_copy
        .read(&Key::Balance(purse.addr()))
        .expect("balance read should not fail")
    {
        Some(StoredValue::CLValue(value)) => value.into_t::<U512>().unwrap(),
        Some(other) => panic!("unexpected balance value: {other:?}"),
        None => U512::zero(),
    }
}

fn seed_evm_balance<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
    balance: U512,
) {
    let main_purse = evm::deterministic_purse(address);
    tracking_copy.write(
        Key::Evm(evm::EvmAddr::Account(address)),
        StoredValue::CLValue(CLValue::from_t(Key::URef(main_purse)).unwrap()),
    );
    tracking_copy.write(
        Key::Evm(evm::EvmAddr::Nonce(address)),
        StoredValue::CLValue(CLValue::from_t(0u64).unwrap()),
    );
    tracking_copy.write(
        Key::Evm(evm::EvmAddr::CodeHash(address)),
        StoredValue::CLValue(CLValue::from_t(EMPTY_CODE_HASH).unwrap()),
    );
    tracking_copy.write(
        Key::Balance(main_purse.addr()),
        StoredValue::CLValue(CLValue::from_t(balance).unwrap()),
    );
}

fn read_evm_nonce<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
) -> u64 {
    match tracking_copy
        .read(&Key::Evm(evm::EvmAddr::Nonce(address)))
        .expect("nonce read should not fail")
    {
        Some(StoredValue::CLValue(value)) => value.into_t::<u64>().unwrap(),
        Some(other) => panic!("unexpected nonce value: {other:?}"),
        None => 0,
    }
}

fn read_code_hash<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
) -> evm::Hash {
    match tracking_copy
        .read(&Key::Evm(evm::EvmAddr::CodeHash(address)))
        .expect("code hash read should not fail")
    {
        Some(StoredValue::CLValue(value)) => value.into_t::<evm::Hash>().unwrap(),
        Some(other) => panic!("unexpected code hash value: {other:?}"),
        None => EMPTY_CODE_HASH,
    }
}

fn read_code<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tracking_copy: &mut TrackingCopy<R>,
    code_hash: evm::Hash,
) -> Option<Vec<u8>> {
    match tracking_copy
        .read(&Key::Evm(evm::EvmAddr::ByteCode(code_hash)))
        .expect("bytecode read should not fail")
    {
        Some(StoredValue::ByteCode(byte_code)) => Some(byte_code.bytes().to_vec()),
        Some(other) => panic!("unexpected bytecode value: {other:?}"),
        None => None,
    }
}

fn delegation_code(delegate: evm::Address) -> Vec<u8> {
    let mut code = vec![0xef, 0x01, 0x00];
    code.extend_from_slice(delegate.as_bytes());
    code
}

#[test]
fn blockhash_uses_supplied_provider() {
    let executor = executor(evm::EvmSpec::Prague);
    let from = evm::Address::new([1; 20]);
    let (mut tracking_copy, _tempdir) = tracking_copy();
    let contract = execute_call(
        &executor,
        &mut tracking_copy,
        from,
        None,
        blockhash_contract_init_code(),
    )
    .created_contract_address
    .expect("deploy should return a contract address");
    let block_hash_provider = HeightBlockHashProvider;

    let outcome = executor
        .execute_with_block_hash_provider(
            &mut tracking_copy,
            call_request(from, Some(contract), Vec::new(), CasperU256::zero()),
            &block_hash_provider,
        )
        .expect("EVM execution should succeed");
    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(outcome.output.as_slice(), &[0u8; evm::HASH_LENGTH]);

    let mut too_old_request = call_request(from, Some(contract), Vec::new(), CasperU256::zero());
    too_old_request.block.number = 258;
    let outcome = executor
        .execute_with_block_hash_provider(&mut tracking_copy, too_old_request, &block_hash_provider)
        .expect("EVM execution should succeed");
    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(outcome.output.as_slice(), &[0u8; evm::HASH_LENGTH]);

    let mut historical_request = call_request(from, Some(contract), Vec::new(), CasperU256::zero());
    historical_request.block.number = 2;
    let outcome = executor
        .execute_with_block_hash_provider(
            &mut tracking_copy,
            historical_request,
            &block_hash_provider,
        )
        .expect("EVM execution should succeed");

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(outcome.output.as_slice(), block_hash_for_height(1).as_ref());
}

#[test]
fn eip7702_authorization_installs_delegation_and_executes_delegate_code() {
    let executor = executor(evm::EvmSpec::Prague);
    let deployer = evm::Address::new([1; 20]);
    let authority = authorization_authority();
    let (mut tracking_copy, _tempdir) = tracking_copy();
    let delegate = deploy_code(
        &executor,
        &mut tracking_copy,
        deployer,
        return_word_contract_init_code(42),
    );
    let (transaction, recovered_authority) =
        eip7702_transaction(authority, delegate, 0, 0, Vec::new());
    assert_eq!(recovered_authority, authority);
    seed_evm_balance(
        &mut tracking_copy,
        transaction.from(),
        U512::from(1_000_000_000u64),
    );
    seed_evm_balance(&mut tracking_copy, authority, U512::zero());

    let outcome = execute_transaction(&executor, &mut tracking_copy, transaction);

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(decode_word(&outcome.output), 42);
    let code_hash = read_code_hash(&mut tracking_copy, authority);
    assert_ne!(code_hash, EMPTY_CODE_HASH);
    assert_eq!(
        read_code(&mut tracking_copy, code_hash),
        Some(delegation_code(delegate))
    );
}

#[test]
fn eip7702_missing_priority_fee_defaults_to_zero_for_execution() {
    let executor = executor(evm::EvmSpec::Prague);
    let deployer = evm::Address::new([1; 20]);
    let authority = authorization_authority();
    let (mut tracking_copy, _tempdir) = tracking_copy();
    let delegate = deploy_code(
        &executor,
        &mut tracking_copy,
        deployer,
        return_word_contract_init_code(42),
    );
    let (transaction, _) = eip7702_transaction(authority, delegate, 0, 0, Vec::new());
    let transaction = eip7702_transaction_without_priority_fee(transaction);
    seed_evm_balance(
        &mut tracking_copy,
        transaction.from(),
        U512::from(1_000_000_000u64),
    );
    seed_evm_balance(&mut tracking_copy, authority, U512::zero());

    let outcome = execute_transaction(&executor, &mut tracking_copy, transaction);

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(decode_word(&outcome.output), 42);
}

#[test]
fn eip7702_delegation_persists_when_call_reverts() {
    let executor = executor(evm::EvmSpec::Prague);
    let deployer = evm::Address::new([1; 20]);
    let authority = authorization_authority();
    let (mut tracking_copy, _tempdir) = tracking_copy();
    let delegate = deploy_code(
        &executor,
        &mut tracking_copy,
        deployer,
        reverting_contract_init_code(),
    );
    let (transaction, _) = eip7702_transaction(authority, delegate, 0, 0, Vec::new());
    seed_evm_balance(
        &mut tracking_copy,
        transaction.from(),
        U512::from(1_000_000_000u64),
    );
    seed_evm_balance(&mut tracking_copy, authority, U512::zero());

    let outcome = execute_transaction(&executor, &mut tracking_copy, transaction);

    assert_eq!(outcome.status, ExecutionStatus::Revert);
    let code_hash = read_code_hash(&mut tracking_copy, authority);
    assert_eq!(
        read_code(&mut tracking_copy, code_hash),
        Some(delegation_code(delegate))
    );
}

#[test]
fn eip7702_stale_authorization_is_skipped() {
    let executor = executor(evm::EvmSpec::Prague);
    let deployer = evm::Address::new([1; 20]);
    let authority = authorization_authority();
    let (mut tracking_copy, _tempdir) = tracking_copy();
    let delegate = deploy_code(
        &executor,
        &mut tracking_copy,
        deployer,
        return_word_contract_init_code(42),
    );
    let (transaction, _) = eip7702_transaction(authority, delegate, 1, 0, Vec::new());
    seed_evm_balance(
        &mut tracking_copy,
        transaction.from(),
        U512::from(1_000_000_000u64),
    );
    seed_evm_balance(&mut tracking_copy, authority, U512::zero());

    let outcome = execute_transaction(&executor, &mut tracking_copy, transaction);

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert!(outcome.output.is_empty());
    assert_eq!(read_evm_nonce(&mut tracking_copy, authority), 0);
    assert_eq!(
        read_code_hash(&mut tracking_copy, authority),
        EMPTY_CODE_HASH
    );
}

#[test]
fn eip7702_zero_address_authorization_clears_delegation() {
    let executor = executor(evm::EvmSpec::Prague);
    let deployer = evm::Address::new([1; 20]);
    let authority = authorization_authority();
    let (mut tracking_copy, _tempdir) = tracking_copy();
    let delegate = deploy_code(
        &executor,
        &mut tracking_copy,
        deployer,
        return_word_contract_init_code(42),
    );
    let (transaction, _) = eip7702_transaction(authority, delegate, 0, 0, Vec::new());
    seed_evm_balance(
        &mut tracking_copy,
        transaction.from(),
        U512::from(1_000_000_000u64),
    );
    seed_evm_balance(&mut tracking_copy, authority, U512::zero());
    let outcome = execute_transaction(&executor, &mut tracking_copy, transaction);
    assert_eq!(outcome.status, ExecutionStatus::Success);

    let (clear_transaction, _) =
        eip7702_transaction(authority, evm::Address::ZERO, 1, 1, Vec::new());
    let outcome = execute_transaction(&executor, &mut tracking_copy, clear_transaction);

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(
        read_code_hash(&mut tracking_copy, authority),
        EMPTY_CODE_HASH
    );
}

#[test]
fn counter_supports_committed_and_discarded_execution() {
    let executor = executor(evm::EvmSpec::Prague);
    let from = evm::Address::new([1; 20]);
    let (mut tracking_copy, _tempdir) = tracking_copy();
    let counter = deploy(&executor, &mut tracking_copy, from, "Counter");

    let increment = execute_call(
        &executor,
        &mut tracking_copy,
        from,
        Some(counter),
        selector("increment()"),
    );
    assert_eq!(decode_word(&increment.output), 1);

    let mut view = tracking_copy.fork();
    let view_increment = execute_call(
        &executor,
        &mut view,
        from,
        Some(counter),
        selector("increment()"),
    );
    assert_eq!(decode_word(&view_increment.output), 2);

    let get = execute_call(
        &executor,
        &mut tracking_copy,
        from,
        Some(counter),
        selector("get()"),
    );
    assert_eq!(decode_word(&get.output), 1);
}

#[test]
fn erc20_and_native_purse_balances_update() {
    let executor = executor(evm::EvmSpec::Prague);
    let owner = evm::Address::new([1; 20]);
    let recipient = evm::Address::new([2; 20]);
    let spender = evm::Address::new([3; 20]);
    let (mut tracking_copy, _tempdir) = tracking_copy();

    seed_evm_balance(&mut tracking_copy, owner, U512::from(1_000u64));
    let transfer_value = CasperU256::from(250);
    let outcome = executor
        .execute(
            &mut tracking_copy,
            call_request(owner, Some(recipient), Vec::new(), transfer_value),
        )
        .expect("native EVM transfer should succeed");
    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(read_balance(&mut tracking_copy, owner), U512::from(750u64));
    assert_eq!(
        read_balance(&mut tracking_copy, recipient),
        U512::from(250u64)
    );

    let token = deploy(&executor, &mut tracking_copy, owner, "MinimalERC20");
    execute_call(
        &executor,
        &mut tracking_copy,
        owner,
        Some(token),
        calldata("mint(address,uint256)", &[address_word(owner), word(1_000)]),
    );
    execute_call(
        &executor,
        &mut tracking_copy,
        owner,
        Some(token),
        calldata(
            "transfer(address,uint256)",
            &[address_word(recipient), word(150)],
        ),
    );
    execute_call(
        &executor,
        &mut tracking_copy,
        owner,
        Some(token),
        calldata(
            "approve(address,uint256)",
            &[address_word(spender), word(100)],
        ),
    );
    execute_call(
        &executor,
        &mut tracking_copy,
        spender,
        Some(token),
        calldata(
            "transferFrom(address,address,uint256)",
            &[address_word(owner), address_word(recipient), word(40)],
        ),
    );

    let owner_balance = execute_call(
        &executor,
        &mut tracking_copy,
        owner,
        Some(token),
        calldata("balanceOf(address)", &[address_word(owner)]),
    );
    let recipient_balance = execute_call(
        &executor,
        &mut tracking_copy,
        owner,
        Some(token),
        calldata("balanceOf(address)", &[address_word(recipient)]),
    );
    let remaining_allowance = execute_call(
        &executor,
        &mut tracking_copy,
        owner,
        Some(token),
        calldata(
            "allowance(address,address)",
            &[address_word(owner), address_word(spender)],
        ),
    );

    assert_eq!(decode_word(&owner_balance.output), 810);
    assert_eq!(decode_word(&recipient_balance.output), 190);
    assert_eq!(decode_word(&remaining_allowance.output), 60);
}

#[test]
fn nonzero_gas_price_does_not_charge_evm_balances() {
    let executor = executor(evm::EvmSpec::Prague);
    let sender = evm::Address::new([1; 20]);
    let recipient = evm::Address::new([2; 20]);
    let beneficiary = evm::Address::new([3; 20]);
    let (mut tracking_copy, _tempdir) = tracking_copy();
    let initial_balance = U512::from(10_000_000u64);
    let transfer_value = CasperU256::from(250u64);

    seed_evm_balance(&mut tracking_copy, sender, initial_balance);
    let mut block_context = block();
    block_context.beneficiary = beneficiary;
    let request = ExecuteRequest {
        block: block_context,
        kind: ExecuteKind::Call(CallRequest {
            from: sender,
            to: Some(recipient),
            value: transfer_value,
            input: Vec::new(),
            gas_limit: 100_000,
            gas_price: 2,
            nonce: 0,
            validation: CallValidation::Checked,
        }),
    };

    let outcome = executor
        .execute(&mut tracking_copy, request)
        .expect("native EVM transfer should succeed");

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(
        read_balance(&mut tracking_copy, sender),
        initial_balance - U512::from(250u64)
    );
    assert_eq!(
        read_balance(&mut tracking_copy, recipient),
        U512::from(250u64)
    );
    assert_eq!(read_balance(&mut tracking_copy, beneficiary), U512::zero());
}

#[test]
fn erc721_mint_approve_and_transfer() {
    let executor = executor(evm::EvmSpec::Prague);
    let owner = evm::Address::new([1; 20]);
    let recipient = evm::Address::new([2; 20]);
    let approved = evm::Address::new([3; 20]);
    let (mut tracking_copy, _tempdir) = tracking_copy();
    let nft = deploy(&executor, &mut tracking_copy, owner, "MinimalERC721");

    execute_call(
        &executor,
        &mut tracking_copy,
        owner,
        Some(nft),
        calldata("mint(address,uint256)", &[address_word(owner), word(42)]),
    );
    let initial_owner = execute_call(
        &executor,
        &mut tracking_copy,
        owner,
        Some(nft),
        calldata("ownerOf(uint256)", &[word(42)]),
    );
    assert_eq!(decode_address(&initial_owner.output), owner);

    execute_call(
        &executor,
        &mut tracking_copy,
        owner,
        Some(nft),
        calldata(
            "approve(address,uint256)",
            &[address_word(approved), word(42)],
        ),
    );
    execute_call(
        &executor,
        &mut tracking_copy,
        approved,
        Some(nft),
        calldata(
            "transferFrom(address,address,uint256)",
            &[address_word(owner), address_word(recipient), word(42)],
        ),
    );
    let final_owner = execute_call(
        &executor,
        &mut tracking_copy,
        owner,
        Some(nft),
        calldata("ownerOf(uint256)", &[word(42)]),
    );
    assert_eq!(decode_address(&final_owner.output), recipient);
}

#[test]
fn storage_zeroes_are_pruned() {
    let executor = executor(evm::EvmSpec::Prague);
    let from = evm::Address::new([1; 20]);
    let (mut tracking_copy, _tempdir) = tracking_copy();
    let contract = deploy(&executor, &mut tracking_copy, from, "StorageDelete");

    execute_call(
        &executor,
        &mut tracking_copy,
        from,
        Some(contract),
        calldata("set(uint256)", &[word(123)]),
    );
    assert_eq!(
        read_storage(&mut tracking_copy, contract, CasperU256::zero()),
        Some(storage_word(123))
    );

    execute_call(
        &executor,
        &mut tracking_copy,
        from,
        Some(contract),
        selector("clear()"),
    );
    assert_eq!(
        read_storage(&mut tracking_copy, contract, CasperU256::zero()),
        None
    );
}

#[test]
fn selfdestruct_preserves_account_on_prague() {
    let from = evm::Address::new([1; 20]);
    let beneficiary = evm::Address::new([2; 20]);

    let prague_executor = executor(evm::EvmSpec::Prague);
    let (mut prague_tracking_copy, _prague_tempdir) = tracking_copy();
    let prague_contract = deploy(
        &prague_executor,
        &mut prague_tracking_copy,
        from,
        "SelfDestruct",
    );
    execute_call(
        &prague_executor,
        &mut prague_tracking_copy,
        from,
        Some(prague_contract),
        calldata("destroy(address)", &[address_word(beneficiary)]),
    );
    assert!(prague_tracking_copy
        .read(&Key::Evm(evm::EvmAddr::Account(prague_contract)))
        .unwrap()
        .is_some());
}

#[test]
fn signed_transactions_require_configured_chain_id() {
    let executor = executor(evm::EvmSpec::Prague);
    let (mut tracking_copy, _tempdir) = tracking_copy();
    let missing_chain_id = legacy_transaction_without_chain_id();
    let request = ExecuteRequest {
        block: block(),
        kind: ExecuteKind::Transaction(missing_chain_id),
    };
    assert!(matches!(
        executor.execute(&mut tracking_copy, request),
        Err(Error::MissingChainId)
    ));

    let wrong_chain_executor = EvmExecutor::new(evm::EvmConfig {
        enabled: true,
        chain_id: 8,
        spec: evm::EvmSpec::Prague,
        block_gas_limit: 30_000_000,
        base_fee: 0,
    });
    let transaction = legacy_transaction(Some(7));
    let request = ExecuteRequest {
        block: block(),
        kind: ExecuteKind::Transaction(transaction),
    };
    assert!(matches!(
        wrong_chain_executor.execute(&mut tracking_copy, request),
        Err(Error::ChainIdMismatch {
            expected: 8,
            actual: 7
        })
    ));
}

#[test]
fn signed_transaction_sender_uses_linked_casper_account_identity() {
    let executor = executor(evm::EvmSpec::Prague);
    let (mut tracking_copy, _tempdir) = tracking_copy();
    let transaction = legacy_transaction(Some(7));
    let signer = transaction
        .signer()
        .expect("transaction should have a signer");
    let account_hash = signer.to_account_hash();
    let main_purse = URef::new([9; 32], AccessRights::READ_ADD_WRITE);
    let initial_balance = U512::from(1_000_000u64);

    tracking_copy.write(
        Key::Account(account_hash),
        StoredValue::Account(Account::create(account_hash, NamedKeys::new(), main_purse)),
    );
    tracking_copy.write(
        Key::Balance(main_purse.addr()),
        StoredValue::CLValue(CLValue::from_t(initial_balance).unwrap()),
    );
    tracking_copy.write(
        Key::Evm(evm::EvmAddr::Account(transaction.from())),
        StoredValue::CLValue(CLValue::from_t(Key::Account(account_hash)).unwrap()),
    );

    let request = ExecuteRequest {
        block: block(),
        kind: ExecuteKind::Transaction(transaction.clone()),
    };
    let outcome = executor
        .execute(&mut tracking_copy, request)
        .expect("EVM execution should succeed");

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(read_evm_nonce(&mut tracking_copy, transaction.from()), 1);
    assert_eq!(
        read_balance(&mut tracking_copy, transaction.from()),
        initial_balance
    );
    match tracking_copy
        .read(&Key::Evm(evm::EvmAddr::Account(transaction.from())))
        .expect("identity read should not fail")
    {
        Some(StoredValue::CLValue(value)) => {
            assert_eq!(value.into_t::<Key>().unwrap(), Key::Account(account_hash));
        }
        other => panic!("unexpected EVM identity value: {other:?}"),
    }
}

#[test]
fn signed_transaction_sender_keeps_evm_native_identity() {
    let executor = executor(evm::EvmSpec::Prague);
    let (mut tracking_copy, _tempdir) = tracking_copy();
    let transaction = legacy_transaction(Some(7));
    let signer = transaction
        .signer()
        .expect("transaction should have a signer");
    let account_hash = signer.to_account_hash();
    let initial_balance = U512::from(1_000_000u64);

    seed_evm_balance(&mut tracking_copy, transaction.from(), initial_balance);

    let request = ExecuteRequest {
        block: block(),
        kind: ExecuteKind::Transaction(transaction.clone()),
    };
    let outcome = executor
        .execute(&mut tracking_copy, request)
        .expect("EVM execution should succeed");

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(read_evm_nonce(&mut tracking_copy, transaction.from()), 1);
    assert_eq!(
        read_balance(&mut tracking_copy, transaction.from()),
        initial_balance
    );
    assert_eq!(
        tracking_copy
            .read(&Key::Account(account_hash))
            .expect("account read should not fail"),
        None
    );
    match tracking_copy
        .read(&Key::Evm(evm::EvmAddr::Account(transaction.from())))
        .expect("identity read should not fail")
    {
        Some(StoredValue::CLValue(value)) => {
            assert_eq!(
                value.into_t::<Key>().unwrap(),
                Key::URef(evm::deterministic_purse(transaction.from()))
            );
        }
        other => panic!("unexpected EVM identity value: {other:?}"),
    }
}

#[test]
fn checked_calls_enforce_transaction_validation() {
    let executor = executor(evm::EvmSpec::Prague);
    let from = evm::Address::new([1; 20]);
    let recipient = evm::Address::new([2; 20]);
    let (mut tracking_copy, _tempdir) = tracking_copy();

    let mut request = checked_call_request(from, Some(recipient), Vec::new(), CasperU256::zero());
    request.block.base_fee = Some(1);
    assert!(matches!(
        executor.execute(&mut tracking_copy, request),
        Err(Error::Revm(_))
    ));
}
