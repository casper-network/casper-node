use std::path::PathBuf;

use alloy_consensus::{crypto::secp256k1, SignableTransaction, TxEip7702, TxEnvelope, TxLegacy};
use alloy_eips::{
    eip2718::Encodable2718,
    eip7702::{
        Authorization as AlloyAuthorization, SignedAuthorization as AlloySignedAuthorization,
    },
};
use alloy_primitives::{keccak256, Address as AlloyAddress, Signature, TxKind, B256, U256};
use casper_executor_evm::{
    BlockContext, CallRequest, CallValidation, DbError, Error, EvmExecutor, ExecuteKind,
    ExecuteRequest, ExecutionStatus, SystemCallRequest, BLOCK_HASH_HISTORY, EMPTY_CODE_HASH,
};
use casper_storage::{
    block_store::{lmdb::LmdbBlockStore, BlockStoreTransaction},
    data_access_layer::{DataAccessLayer, GenesisRequest, GenesisResult},
    eip2935, eip4788,
    global_state::{
        self,
        error::Error as GlobalStateError,
        state::{
            lmdb::{LmdbGlobalState, LmdbGlobalStateView},
            CommitProvider, StateProvider, StateReader,
        },
    },
    TrackingCopy,
};
use casper_types::{
    contracts::NamedKeys, evm, AccessRights, Account, BlockHash, BlockHeader, BlockHeaderV2,
    ByteCode, ByteCodeKind, CLValue, ChainspecRegistry, Digest, EraId, EvmAddr, EvmConfig, EvmSpec,
    EvmTransaction, GenesisAccount, GenesisConfig, HoldBalanceHandling, Key, Motes,
    ProtocolVersion, PublicKey, SecretKey, StorageCosts, StoredValue, SystemConfig, Timestamp,
    URef, WasmConfig, DEFAULT_WEI_PER_MOTE, U256 as CasperU256, U512,
};
use once_cell::sync::OnceCell;
use revm::bytecode::opcode;

const SIGNING_SECRET: [u8; 32] = [7; 32];
const AUTHORIZATION_SECRET: [u8; 32] = [8; 32];

fn tracking_copy() -> (
    TrackingCopy<LmdbGlobalStateView>,
    DataAccessLayer<LmdbGlobalState>,
    impl Send,
) {
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
        EvmConfig::default(),
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
    let block_store =
        LmdbBlockStore::new_temporary(64 * 1024 * 1024).expect("should create block store");
    let data_access_layer = DataAccessLayer {
        block_store,
        state: global_state,
        max_query_depth: 5,
        enable_addressable_entity: false,
    };
    (
        TrackingCopy::new(reader, 5, false),
        data_access_layer,
        tempdir,
    )
}

fn executor(spec: EvmSpec) -> EvmExecutor {
    EvmExecutor::new(EvmConfig {
        enabled: true,
        chain_id: 7,
        spec,
        block_gas_limit: 30_000_000,
        base_fee: 0,
        wei_per_mote: DEFAULT_WEI_PER_MOTE,
    })
}

fn block() -> BlockContext {
    BlockContext {
        number: 1,
        timestamp: 1_714_000_000,
        beneficiary: evm::Address::ZERO,
        gas_limit: None,
        base_fee: None,
        prevrandao: evm::Hash::new([0x99; evm::HASH_LENGTH]),
    }
}

fn block_header(block_height: u64) -> BlockHeader {
    let proposer = PublicKey::from(
        &SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH])
            .expect("should create secret key"),
    );
    BlockHeader::V2(BlockHeaderV2::new(
        BlockHash::new(Digest::hash("parent block")),
        Digest::hash("state root"),
        Digest::hash("body"),
        false,
        Digest::hash("accumulated seed"),
        None,
        Timestamp::from(1_714_000_000),
        EraId::new(1),
        block_height,
        ProtocolVersion::V2_0_0,
        proposer,
        1,
        None,
        OnceCell::new(),
    ))
}

fn write_block_header(
    data_access_layer: &DataAccessLayer<LmdbGlobalState>,
    block_header: &BlockHeader,
) {
    let mut block_store = data_access_layer.block_store.clone();
    let mut transaction = block_store
        .checkout_rw()
        .expect("should check out write transaction");
    transaction
        .write_block_header(block_header)
        .expect("should write block header");
    transaction.commit().expect("should commit block header");
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

fn prevrandao_contract_init_code() -> Vec<u8> {
    let runtime = vec![
        // bytes32 value = prevrandao();
        opcode::DIFFICULTY,
        // mstore(0, value);
        opcode::PUSH1,
        0,
        opcode::MSTORE,
        // return abi.encode(value);
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

fn reverting_runtime() -> Vec<u8> {
    vec![opcode::PUSH1, 0, opcode::PUSH1, 0, opcode::REVERT]
}

fn coinbase_transfer_init_code() -> Vec<u8> {
    let revert_offset = 19u8;
    let runtime = vec![
        opcode::PUSH1,
        0, // return size
        opcode::PUSH1,
        0, // return offset
        opcode::PUSH1,
        0, // calldata size
        opcode::PUSH1,
        0, // calldata offset
        opcode::CALLVALUE,
        opcode::COINBASE,
        opcode::PUSH2,
        0x08,
        0xfc, // 2300 gas
        opcode::CALL,
        opcode::ISZERO,
        opcode::PUSH1,
        revert_offset,
        opcode::JUMPI,
        opcode::STOP,
        opcode::JUMPDEST,
        opcode::PUSH1,
        0,
        opcode::PUSH1,
        0,
        opcode::REVERT,
    ];
    init_code_returning(runtime)
}

fn coinbase_observer_init_code() -> Vec<u8> {
    init_code_returning(vec![opcode::COINBASE, opcode::POP, opcode::STOP])
}

fn value_and_balance_observer_init_code() -> Vec<u8> {
    let runtime = vec![
        opcode::CALLVALUE,
        opcode::PUSH1,
        0,
        opcode::MSTORE,
        opcode::ADDRESS,
        opcode::BALANCE,
        opcode::PUSH1,
        32,
        opcode::MSTORE,
        opcode::SELFBALANCE,
        opcode::PUSH1,
        64,
        opcode::MSTORE,
        opcode::PUSH1,
        96,
        opcode::PUSH1,
        0,
        opcode::RETURN,
    ];
    init_code_returning(runtime)
}

fn append_one_wei_call(runtime: &mut Vec<u8>, recipient: evm::Address) {
    runtime.extend([
        opcode::PUSH1,
        0, // return size
        opcode::PUSH1,
        0, // return offset
        opcode::PUSH1,
        0, // calldata size
        opcode::PUSH1,
        0, // calldata offset
        opcode::PUSH1,
        1, // value
        opcode::PUSH20,
    ]);
    runtime.extend_from_slice(recipient.as_bytes());
    runtime.extend([
        opcode::PUSH2,
        0xff,
        0xff, // gas
        opcode::CALL,
        opcode::POP,
    ]);
}

fn one_wei_transfer_init_code(recipient: evm::Address, terminal: &[u8]) -> Vec<u8> {
    let mut runtime = Vec::new();
    append_one_wei_call(&mut runtime, recipient);
    runtime.extend_from_slice(terminal);
    init_code_returning(runtime)
}

fn return_call_value_to_caller_init_code() -> Vec<u8> {
    let runtime = vec![
        opcode::PUSH1,
        0, // return size
        opcode::PUSH1,
        0, // return offset
        opcode::PUSH1,
        0, // calldata size
        opcode::PUSH1,
        0, // calldata offset
        opcode::CALLVALUE,
        opcode::CALLER,
        opcode::PUSH2,
        0xff,
        0xff, // gas
        opcode::CALL,
        opcode::POP,
        opcode::STOP,
    ];
    init_code_returning(runtime)
}

fn call_request(
    from: evm::Address,
    to: Option<evm::Address>,
    input: Vec<u8>,
    value_motes: CasperU256,
) -> ExecuteRequest {
    call_request_wei(
        from,
        to,
        input,
        value_motes * CasperU256::from(DEFAULT_WEI_PER_MOTE),
    )
}

fn call_request_wei(
    from: evm::Address,
    to: Option<evm::Address>,
    input: Vec<u8>,
    value_wei: CasperU256,
) -> ExecuteRequest {
    ExecuteRequest {
        block: block(),
        kind: ExecuteKind::Call(CallRequest {
            from,
            to,
            value: value_wei,
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
    value_motes: CasperU256,
) -> ExecuteRequest {
    ExecuteRequest {
        block: block(),
        kind: ExecuteKind::Call(CallRequest {
            from,
            to,
            value: value_motes * CasperU256::from(DEFAULT_WEI_PER_MOTE),
            input,
            gas_limit: 5_000_000,
            gas_price: 0,
            nonce: 0,
            validation: CallValidation::Checked,
        }),
    }
}

fn execute_call<R, S>(
    executor: &EvmExecutor,
    data_access_layer: &DataAccessLayer<S>,
    tracking_copy: &mut TrackingCopy<R>,
    from: evm::Address,
    to: Option<evm::Address>,
    input: Vec<u8>,
) -> casper_executor_evm::ExecutionOutcome
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    let outcome = executor
        .execute(
            data_access_layer,
            tracking_copy,
            call_request(from, to, input, CasperU256::zero()),
        )
        .expect("EVM execution should succeed");
    assert_eq!(outcome.status, ExecutionStatus::Success);
    outcome
}

fn execute_transaction<R, S>(
    executor: &EvmExecutor,
    data_access_layer: &DataAccessLayer<S>,
    tracking_copy: &mut TrackingCopy<R>,
    transaction: EvmTransaction,
) -> casper_executor_evm::ExecutionOutcome
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    executor
        .execute(
            data_access_layer,
            tracking_copy,
            ExecuteRequest {
                block: block(),
                kind: ExecuteKind::Transaction(Box::new(transaction)),
            },
        )
        .expect("EVM transaction execution should succeed")
}

fn deploy_code<R, S>(
    executor: &EvmExecutor,
    data_access_layer: &DataAccessLayer<S>,
    tracking_copy: &mut TrackingCopy<R>,
    from: evm::Address,
    code: Vec<u8>,
) -> evm::Address
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    execute_call(executor, data_access_layer, tracking_copy, from, None, code)
        .created_contract_address
        .expect("deploy should return a contract address")
}

fn deploy<R, S>(
    executor: &EvmExecutor,
    data_access_layer: &DataAccessLayer<S>,
    tracking_copy: &mut TrackingCopy<R>,
    from: evm::Address,
    name: &str,
) -> evm::Address
where
    R: StateReader<Key, StoredValue, Error = GlobalStateError>,
{
    execute_call(
        executor,
        data_access_layer,
        tracking_copy,
        from,
        None,
        contract_bin(name),
    )
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

fn legacy_transaction(chain_id: Option<u64>) -> EvmTransaction {
    legacy_transaction_with_value(chain_id, U256::ZERO)
}

fn legacy_transaction_with_value(chain_id: Option<u64>, value: U256) -> EvmTransaction {
    legacy_transaction_to(chain_id, AlloyAddress::from([1u8; 20]), value, 21_000)
}

fn legacy_transaction_to(
    chain_id: Option<u64>,
    to: AlloyAddress,
    value: U256,
    gas_limit: u64,
) -> EvmTransaction {
    let tx = TxLegacy {
        chain_id,
        nonce: 0,
        gas_price: 1,
        gas_limit,
        to: TxKind::Call(to),
        value,
        input: Default::default(),
    };
    let tx = tx.into_signed(Signature::test_signature().with_parity(true));
    let envelope: TxEnvelope = tx.into();
    EvmTransaction::from_signed_rlp(
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
) -> (EvmTransaction, evm::Address) {
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
    let transaction = EvmTransaction::from_signed_rlp(
        envelope.encoded_2718(),
        Timestamp::zero(),
        casper_types::TimeDiff::from_seconds(60),
    )
    .expect("transaction should decode");
    (transaction, authority)
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

fn legacy_transaction_without_chain_id() -> EvmTransaction {
    legacy_transaction(None)
}

fn read_storage<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
    slot: CasperU256,
) -> Option<CasperU256> {
    match tracking_copy
        .read(&Key::Evm(EvmAddr::Storage(evm::StorageAddr::new(
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
        .read(&Key::Evm(EvmAddr::Account(address)))
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

fn read_account_balance<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tracking_copy: &mut TrackingCopy<R>,
    account_hash: casper_types::account::AccountHash,
) -> U512 {
    let main_purse = match tracking_copy
        .read(&Key::Account(account_hash))
        .expect("account read should not fail")
    {
        Some(StoredValue::Account(account)) => account.main_purse(),
        Some(other) => panic!("unexpected account value: {other:?}"),
        None => return U512::zero(),
    };
    match tracking_copy
        .read(&Key::Balance(main_purse.addr()))
        .expect("balance read should not fail")
    {
        Some(StoredValue::CLValue(value)) => value.into_t::<U512>().unwrap(),
        Some(other) => panic!("unexpected balance value: {other:?}"),
        None => U512::zero(),
    }
}

fn read_evm_identity<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
) -> Option<Key> {
    match tracking_copy
        .read(&Key::Evm(EvmAddr::Account(address)))
        .expect("identity read should not fail")
    {
        Some(StoredValue::CLValue(value)) => Some(value.into_t::<Key>().unwrap()),
        Some(other) => panic!("unexpected EVM identity value: {other:?}"),
        None => None,
    }
}

fn seed_evm_balance<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
    balance: U512,
) {
    let main_purse = evm::deterministic_purse(address);
    tracking_copy.write(
        Key::Evm(EvmAddr::Account(address)),
        StoredValue::CLValue(CLValue::from_t(Key::URef(main_purse)).unwrap()),
    );
    tracking_copy.write(
        Key::Evm(EvmAddr::Nonce(address)),
        StoredValue::CLValue(CLValue::from_t(0u64).unwrap()),
    );
    tracking_copy.write(
        Key::Evm(EvmAddr::CodeHash(address)),
        StoredValue::CLValue(CLValue::from_t(EMPTY_CODE_HASH).unwrap()),
    );
    tracking_copy.write(
        Key::Balance(main_purse.addr()),
        StoredValue::CLValue(CLValue::from_t(balance).unwrap()),
    );
}

fn write_existing_evm_balance<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
    balance: U512,
) {
    let main_purse = match read_evm_identity(tracking_copy, address)
        .expect("EVM account should have an identity")
    {
        Key::URef(main_purse) => main_purse,
        Key::Account(account_hash) => match tracking_copy
            .read(&Key::Account(account_hash))
            .expect("linked account read should not fail")
        {
            Some(StoredValue::Account(account)) => account.main_purse(),
            other => panic!("unexpected linked account value: {other:?}"),
        },
        other => panic!("unexpected EVM account identity key: {other:?}"),
    };
    tracking_copy.write(
        Key::Balance(main_purse.addr()),
        StoredValue::CLValue(CLValue::from_t(balance).unwrap()),
    );
}

fn seed_account<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tracking_copy: &mut TrackingCopy<R>,
    account_hash: casper_types::account::AccountHash,
    main_purse: URef,
    balance: U512,
) {
    tracking_copy.write(
        Key::Account(account_hash),
        StoredValue::Account(Account::create(account_hash, NamedKeys::new(), main_purse)),
    );
    tracking_copy.write(
        Key::Balance(main_purse.addr()),
        StoredValue::CLValue(CLValue::from_t(balance).unwrap()),
    );
}

fn seed_evm_code<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
    code: Vec<u8>,
) {
    let digest = keccak256(&code);
    let mut hash = [0u8; evm::HASH_LENGTH];
    hash.copy_from_slice(digest.as_slice());
    let code_hash = evm::Hash::new(hash);
    tracking_copy.write(
        Key::Evm(EvmAddr::CodeHash(address)),
        StoredValue::CLValue(CLValue::from_t(code_hash).unwrap()),
    );
    tracking_copy.write(
        Key::Evm(EvmAddr::ByteCode(code_hash)),
        StoredValue::ByteCode(ByteCode::new(ByteCodeKind::EvmPrague, code)),
    );
}

fn read_evm_nonce<R: StateReader<Key, StoredValue, Error = GlobalStateError>>(
    tracking_copy: &mut TrackingCopy<R>,
    address: evm::Address,
) -> u64 {
    match tracking_copy
        .read(&Key::Evm(EvmAddr::Nonce(address)))
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
        .read(&Key::Evm(EvmAddr::CodeHash(address)))
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
        .read(&Key::Evm(EvmAddr::ByteCode(code_hash)))
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
fn prague_bls12_g1_add_precompile_delegates_to_revm() {
    let executor = executor(EvmSpec::Prague);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let mut precompile_address = [0; evm::ADDRESS_LENGTH];
    precompile_address[evm::ADDRESS_LENGTH - 1] = 0x0b;

    let outcome = execute_call(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        evm::Address::ZERO,
        Some(evm::Address::new(precompile_address)),
        vec![0; 256],
    );

    assert_eq!(outcome.output, vec![0; 128]);
}

#[test]
fn eip4788_native_shortcut_always_returns_zero() {
    let executor = executor(EvmSpec::Prague);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();

    // The shortcut must bypass the installed bytecode and avoid all state reads.
    seed_evm_code(
        &mut tracking_copy,
        eip4788::BEACON_ROOTS_ADDRESS,
        reverting_runtime(),
    );

    for input in [vec![], word(block().timestamp).to_vec(), vec![0xff; 33]] {
        let query = execute_call(
            &executor,
            &data_access_layer,
            &mut tracking_copy,
            evm::Address::ZERO,
            Some(eip4788::BEACON_ROOTS_ADDRESS),
            input,
        );
        assert_eq!(query.output, vec![0; evm::HASH_LENGTH]);
    }
}

#[test]
fn eip2935_native_lookup_reads_indexed_header_and_bypasses_bytecode() {
    let executor = executor(EvmSpec::Prague);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let header = block_header(1);
    let expected_hash = header.block_hash();
    write_block_header(&data_access_layer, &header);

    // Direct calls must use the native lookup even when the installed code would revert.
    seed_evm_code(
        &mut tracking_copy,
        eip2935::BLOCK_HASH_HISTORY_ADDRESS,
        reverting_runtime(),
    );

    let mut stored_request = call_request(
        evm::Address::ZERO,
        Some(eip2935::BLOCK_HASH_HISTORY_ADDRESS),
        word(1).to_vec(),
        CasperU256::zero(),
    );
    stored_request.block.number = 2;
    let stored = executor
        .execute(&data_access_layer, &mut tracking_copy, stored_request)
        .expect("EIP-2935 lookup should execute");
    assert_eq!(stored.status, ExecutionStatus::Success);
    assert_eq!(stored.output.as_slice(), expected_hash.as_ref());

    let mut missing_request = call_request(
        evm::Address::ZERO,
        Some(eip2935::BLOCK_HASH_HISTORY_ADDRESS),
        word(0).to_vec(),
        CasperU256::zero(),
    );
    missing_request.block.number = 2;
    let missing = executor
        .execute(&data_access_layer, &mut tracking_copy, missing_request)
        .expect("EIP-2935 lookup should execute");
    assert_eq!(missing.status, ExecutionStatus::Success);
    assert_eq!(missing.output.as_slice(), &[0; evm::HASH_LENGTH]);

    let mut oldest_valid_request = call_request(
        evm::Address::ZERO,
        Some(eip2935::BLOCK_HASH_HISTORY_ADDRESS),
        word(1).to_vec(),
        CasperU256::zero(),
    );
    oldest_valid_request.block.number = eip2935::HISTORY_BUFFER_LENGTH + 1;
    let oldest_valid = executor
        .execute(&data_access_layer, &mut tracking_copy, oldest_valid_request)
        .expect("EIP-2935 lookup should execute");
    assert_eq!(oldest_valid.status, ExecutionStatus::Success);
    assert_eq!(oldest_valid.output.as_slice(), expected_hash.as_ref());

    let mut too_old_request = call_request(
        evm::Address::ZERO,
        Some(eip2935::BLOCK_HASH_HISTORY_ADDRESS),
        word(1).to_vec(),
        CasperU256::zero(),
    );
    too_old_request.block.number = eip2935::HISTORY_BUFFER_LENGTH + 2;
    let too_old = executor
        .execute(&data_access_layer, &mut tracking_copy, too_old_request)
        .expect("EIP-2935 lookup should execute");
    assert_eq!(too_old.status, ExecutionStatus::Revert);
}

#[test]
fn eip2935_reverts_for_invalid_requests() {
    let executor = executor(EvmSpec::Prague);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    seed_evm_code(
        &mut tracking_copy,
        eip2935::BLOCK_HASH_HISTORY_ADDRESS,
        eip2935::BLOCK_HASH_HISTORY_CODE.to_vec(),
    );

    let mut oversized_height = [0xff; evm::HASH_LENGTH];
    oversized_height[0] = 1;
    let cases = [
        (1, vec![]),
        (1, vec![0; evm::HASH_LENGTH - 1]),
        (1, vec![0; evm::HASH_LENGTH + 1]),
        (0, word(0).to_vec()),
        (1, word(1).to_vec()),
        (1, word(2).to_vec()),
        (1, oversized_height.to_vec()),
    ];

    for (block_number, input) in cases {
        let mut request = call_request(
            evm::Address::ZERO,
            Some(eip2935::BLOCK_HASH_HISTORY_ADDRESS),
            input,
            CasperU256::zero(),
        );
        request.block.number = block_number;
        let outcome = executor
            .execute(&data_access_layer, &mut tracking_copy, request)
            .expect("EIP-2935 lookup should execute");
        assert_eq!(outcome.status, ExecutionStatus::Revert);
    }
}

#[test]
fn eip2935_preserves_block_store_errors() {
    let executor = executor(EvmSpec::Prague);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    seed_evm_code(
        &mut tracking_copy,
        eip2935::BLOCK_HASH_HISTORY_ADDRESS,
        eip2935::BLOCK_HASH_HISTORY_CODE.to_vec(),
    );

    // Exhaust this environment's LMDB reader slots so the native lookup fails while checking out
    // its short-lived read transaction.
    let read_transactions = (0..512)
        .map(|_| {
            data_access_layer
                .block_store
                .checkout_ro()
                .expect("should check out reader")
        })
        .collect::<Vec<_>>();

    let mut request = call_request(
        evm::Address::ZERO,
        Some(eip2935::BLOCK_HASH_HISTORY_ADDRESS),
        word(1).to_vec(),
        CasperU256::zero(),
    );
    request.block.number = 2;

    let error = executor
        .execute(&data_access_layer, &mut tracking_copy, request)
        .expect_err("exhausted LMDB readers should fail execution");
    assert!(matches!(
        error,
        Error::Database(DbError::BlockHash { height: 1, .. })
    ));

    drop(read_transactions);
}

#[test]
fn blockhash_reads_indexed_header_from_data_access_layer() {
    let executor = executor(EvmSpec::Prague);
    let from = evm::Address::new([1; 20]);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let contract = execute_call(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        from,
        None,
        blockhash_contract_init_code(),
    )
    .created_contract_address
    .expect("deploy should return a contract address");
    let outcome = executor
        .execute(
            &data_access_layer,
            &mut tracking_copy,
            call_request(from, Some(contract), Vec::new(), CasperU256::zero()),
        )
        .expect("EVM execution should succeed");
    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(outcome.output.as_slice(), &[0u8; evm::HASH_LENGTH]);

    let mut future_request = call_request(from, Some(contract), Vec::new(), CasperU256::zero());
    future_request.block.number = 0;
    let outcome = executor
        .execute(&data_access_layer, &mut tracking_copy, future_request)
        .expect("EVM execution should succeed");
    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(outcome.output.as_slice(), &[0u8; evm::HASH_LENGTH]);

    let mut missing_request = call_request(from, Some(contract), Vec::new(), CasperU256::zero());
    missing_request.block.number = 2;
    let outcome = executor
        .execute(&data_access_layer, &mut tracking_copy, missing_request)
        .expect("EVM execution should succeed");
    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(outcome.output.as_slice(), &[0u8; evm::HASH_LENGTH]);

    let header = block_header(1);
    let expected_hash = header.block_hash();
    write_block_header(&data_access_layer, &header);

    let mut historical_request = call_request(from, Some(contract), Vec::new(), CasperU256::zero());
    historical_request.block.number = 2;
    let outcome = executor
        .execute(&data_access_layer, &mut tracking_copy, historical_request)
        .expect("EVM execution should succeed");
    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(outcome.output.as_slice(), expected_hash.as_ref());

    let mut oldest_valid_request =
        call_request(from, Some(contract), Vec::new(), CasperU256::zero());
    oldest_valid_request.block.number = BLOCK_HASH_HISTORY + 1;
    let outcome = executor
        .execute(&data_access_layer, &mut tracking_copy, oldest_valid_request)
        .expect("EVM execution should succeed");
    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(outcome.output.as_slice(), expected_hash.as_ref());

    let mut too_old_request = call_request(from, Some(contract), Vec::new(), CasperU256::zero());
    too_old_request.block.number = BLOCK_HASH_HISTORY + 2;
    let outcome = executor
        .execute(&data_access_layer, &mut tracking_copy, too_old_request)
        .expect("EVM execution should succeed");
    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(outcome.output.as_slice(), &[0u8; evm::HASH_LENGTH]);
}

#[test]
fn eip7702_authorization_installs_delegation_and_executes_delegate_code() {
    let executor = executor(EvmSpec::Prague);
    let deployer = evm::Address::new([1; 20]);
    let authority = authorization_authority();
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let delegate = deploy_code(
        &executor,
        &data_access_layer,
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

    let outcome = execute_transaction(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        transaction,
    );

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
fn eip7702_delegation_persists_when_call_reverts() {
    let executor = executor(EvmSpec::Prague);
    let deployer = evm::Address::new([1; 20]);
    let authority = authorization_authority();
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let delegate = deploy_code(
        &executor,
        &data_access_layer,
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

    let outcome = execute_transaction(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        transaction,
    );

    assert_eq!(outcome.status, ExecutionStatus::Revert);
    let code_hash = read_code_hash(&mut tracking_copy, authority);
    assert_eq!(
        read_code(&mut tracking_copy, code_hash),
        Some(delegation_code(delegate))
    );
}

#[test]
fn eip7702_stale_authorization_is_skipped() {
    let executor = executor(EvmSpec::Prague);
    let deployer = evm::Address::new([1; 20]);
    let authority = authorization_authority();
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let delegate = deploy_code(
        &executor,
        &data_access_layer,
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

    let outcome = execute_transaction(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        transaction,
    );

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
    let executor = executor(EvmSpec::Prague);
    let deployer = evm::Address::new([1; 20]);
    let authority = authorization_authority();
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let delegate = deploy_code(
        &executor,
        &data_access_layer,
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
    let outcome = execute_transaction(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        transaction,
    );
    assert_eq!(outcome.status, ExecutionStatus::Success);

    let (clear_transaction, _) =
        eip7702_transaction(authority, evm::Address::ZERO, 1, 1, Vec::new());
    let outcome = execute_transaction(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        clear_transaction,
    );

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(
        read_code_hash(&mut tracking_copy, authority),
        EMPTY_CODE_HASH
    );
}

#[test]
fn counter_supports_committed_and_discarded_execution() {
    let executor = executor(EvmSpec::Prague);
    let from = evm::Address::new([1; 20]);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let counter = deploy(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        from,
        "Counter",
    );

    let increment = execute_call(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        from,
        Some(counter),
        selector("increment()"),
    );
    assert_eq!(decode_word(&increment.output), 1);

    let mut view = tracking_copy.fork();
    let view_increment = execute_call(
        &executor,
        &data_access_layer,
        &mut view,
        from,
        Some(counter),
        selector("increment()"),
    );
    assert_eq!(decode_word(&view_increment.output), 2);

    let get = execute_call(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        from,
        Some(counter),
        selector("get()"),
    );
    assert_eq!(decode_word(&get.output), 1);
}

#[test]
fn erc20_and_native_purse_balances_update() {
    let executor = executor(EvmSpec::Prague);
    let owner = evm::Address::new([1; 20]);
    let recipient = evm::Address::new([2; 20]);
    let spender = evm::Address::new([3; 20]);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();

    seed_evm_balance(&mut tracking_copy, owner, U512::from(1_000u64));
    let transfer_value = CasperU256::from(250);
    let outcome = executor
        .execute(
            &data_access_layer,
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

    let token = deploy(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        owner,
        "MinimalERC20",
    );
    execute_call(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        owner,
        Some(token),
        calldata("mint(address,uint256)", &[address_word(owner), word(1_000)]),
    );
    execute_call(
        &executor,
        &data_access_layer,
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
        &data_access_layer,
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
        &data_access_layer,
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
        &data_access_layer,
        &mut tracking_copy,
        owner,
        Some(token),
        calldata("balanceOf(address)", &[address_word(owner)]),
    );
    let recipient_balance = execute_call(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        owner,
        Some(token),
        calldata("balanceOf(address)", &[address_word(recipient)]),
    );
    let remaining_allowance = execute_call(
        &executor,
        &data_access_layer,
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
fn whole_mote_value_executes_in_wei_and_persists_without_dust() {
    let executor = EvmExecutor::new(EvmConfig {
        enabled: true,
        chain_id: 7,
        spec: EvmSpec::Prague,
        block_gas_limit: 30_000_000,
        base_fee: 0,
        wei_per_mote: DEFAULT_WEI_PER_MOTE,
    });
    let sender = evm::Address::new([0x31; 20]);
    let recipient = evm::Address::new([0x32; 20]);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let initial_motes = U512::from(100_000_000_000u64);
    let transferred_motes = 15_000_000_000u64;
    let value_wei = CasperU256::from(transferred_motes) * CasperU256::from(DEFAULT_WEI_PER_MOTE);

    seed_evm_balance(&mut tracking_copy, sender, initial_motes);
    let outcome = executor
        .execute(
            &data_access_layer,
            &mut tracking_copy,
            call_request_wei(sender, Some(recipient), Vec::new(), value_wei),
        )
        .expect("whole-mote Ethereum value should execute");

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(outcome.dust_motes, U512::zero());
    assert_eq!(
        read_balance(&mut tracking_copy, sender),
        initial_motes - U512::from(transferred_motes)
    );
    assert_eq!(
        read_balance(&mut tracking_copy, recipient),
        U512::from(transferred_motes)
    );
}

#[test]
fn callvalue_and_balance_opcodes_observe_wei() {
    let executor = executor(EvmSpec::Prague);
    let sender = evm::Address::new([0x33; 20]);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    seed_evm_balance(&mut tracking_copy, sender, U512::from(10u64));
    let contract = deploy_code(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        sender,
        value_and_balance_observer_init_code(),
    );
    let transferred_motes = CasperU256::from(7u64);

    let outcome = executor
        .execute(
            &data_access_layer,
            &mut tracking_copy,
            call_request(sender, Some(contract), Vec::new(), transferred_motes),
        )
        .expect("whole-mote observer call should execute");

    let expected_wei = 7 * DEFAULT_WEI_PER_MOTE;
    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(decode_word(&outcome.output[0..32]), expected_wei);
    assert_eq!(decode_word(&outcome.output[32..64]), expected_wei);
    assert_eq!(decode_word(&outcome.output[64..96]), expected_wei);
    assert_eq!(outcome.dust_motes, U512::zero());
    assert_eq!(read_balance(&mut tracking_copy, sender), U512::from(3u64));
    assert_eq!(read_balance(&mut tracking_copy, contract), U512::from(7u64));

    let fractional = executor
        .execute(
            &data_access_layer,
            &mut tracking_copy,
            call_request_wei(sender, Some(contract), Vec::new(), CasperU256::one()),
        )
        .expect("unchecked call should accept an arbitrary wei value");
    assert_eq!(decode_word(&fractional.output[0..32]), 1);
    assert_eq!(decode_word(&fractional.output[32..64]), expected_wei + 1);
    assert_eq!(decode_word(&fractional.output[64..96]), expected_wei + 1);
    assert_eq!(fractional.dust_motes, U512::one());
    assert_eq!(read_balance(&mut tracking_copy, sender), U512::from(2u64));
    assert_eq!(read_balance(&mut tracking_copy, contract), U512::from(7u64));
}

#[test]
fn signed_transaction_passes_original_wei_value_to_callvalue() {
    let executor = executor(EvmSpec::Prague);
    let deployer = evm::Address::new([0x3c; 20]);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let contract = deploy_code(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        deployer,
        value_and_balance_observer_init_code(),
    );
    let value_wei = U256::from(2 * DEFAULT_WEI_PER_MOTE);
    let transaction =
        legacy_transaction_to(Some(7), to_alloy_address(contract), value_wei, 100_000);
    seed_evm_balance(&mut tracking_copy, transaction.from(), U512::from(10u64));

    let outcome = execute_transaction(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        transaction,
    );

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(
        decode_word(&outcome.output[0..32]),
        2 * DEFAULT_WEI_PER_MOTE
    );
    assert_eq!(outcome.dust_motes, U512::zero());
    assert_eq!(read_balance(&mut tracking_copy, contract), U512::from(2u64));
}

#[test]
fn internal_one_wei_transfer_reports_one_aggregate_dust_mote() {
    let executor = executor(EvmSpec::Prague);
    let sender = evm::Address::new([0x34; 20]);
    let recipient = evm::Address::new([0x35; 20]);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let contract = deploy_code(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        sender,
        one_wei_transfer_init_code(recipient, &[opcode::STOP]),
    );
    write_existing_evm_balance(&mut tracking_copy, contract, U512::one());

    let outcome = executor
        .execute(
            &data_access_layer,
            &mut tracking_copy,
            call_request(sender, Some(contract), Vec::new(), CasperU256::zero()),
        )
        .expect("one-wei internal transfer should execute");

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(outcome.dust_motes, U512::one());
    assert_eq!(read_balance(&mut tracking_copy, contract), U512::zero());
    assert_eq!(read_balance(&mut tracking_copy, recipient), U512::zero());
}

#[test]
fn recombined_internal_wei_produces_no_dust() {
    let executor = executor(EvmSpec::Prague);
    let sender = evm::Address::new([0x36; 20]);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let returning_contract = deploy_code(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        sender,
        return_call_value_to_caller_init_code(),
    );
    let sending_contract = deploy_code(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        sender,
        one_wei_transfer_init_code(returning_contract, &[opcode::STOP]),
    );
    write_existing_evm_balance(&mut tracking_copy, sending_contract, U512::one());

    let outcome = executor
        .execute(
            &data_access_layer,
            &mut tracking_copy,
            call_request(
                sender,
                Some(sending_contract),
                Vec::new(),
                CasperU256::zero(),
            ),
        )
        .expect("round-trip one-wei transfer should execute");

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(outcome.dust_motes, U512::zero());
    assert_eq!(
        read_balance(&mut tracking_copy, sending_contract),
        U512::one()
    );
    assert_eq!(
        read_balance(&mut tracking_copy, returning_contract),
        U512::zero()
    );
}

#[test]
fn reverted_and_halted_transfers_report_no_dust() {
    let executor = executor(EvmSpec::Prague);
    let sender = evm::Address::new([0x37; 20]);
    let recipient = evm::Address::new([0x38; 20]);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let reverting_contract = deploy_code(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        sender,
        one_wei_transfer_init_code(
            recipient,
            &[opcode::PUSH1, 0, opcode::PUSH1, 0, opcode::REVERT],
        ),
    );
    write_existing_evm_balance(&mut tracking_copy, reverting_contract, U512::one());

    let reverted = executor
        .execute(
            &data_access_layer,
            &mut tracking_copy,
            call_request(
                sender,
                Some(reverting_contract),
                Vec::new(),
                CasperU256::zero(),
            ),
        )
        .expect("reverting transfer should produce an outcome");
    assert_eq!(reverted.status, ExecutionStatus::Revert);
    assert_eq!(reverted.dust_motes, U512::zero());
    assert_eq!(
        read_balance(&mut tracking_copy, reverting_contract),
        U512::one()
    );
    assert_eq!(read_balance(&mut tracking_copy, recipient), U512::zero());

    let halting_contract = deploy_code(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        sender,
        one_wei_transfer_init_code(recipient, &[0xfe]),
    );
    write_existing_evm_balance(&mut tracking_copy, halting_contract, U512::one());

    let halted = executor
        .execute(
            &data_access_layer,
            &mut tracking_copy,
            call_request(
                sender,
                Some(halting_contract),
                Vec::new(),
                CasperU256::zero(),
            ),
        )
        .expect("halting transfer should produce an outcome");
    assert!(matches!(halted.status, ExecutionStatus::Halt(_)));
    assert_eq!(halted.dust_motes, U512::zero());
    assert_eq!(
        read_balance(&mut tracking_copy, halting_contract),
        U512::one()
    );
    assert_eq!(read_balance(&mut tracking_copy, recipient), U512::zero());
}

#[test]
fn scaled_balance_overflow_is_reported() {
    let executor = executor(EvmSpec::Prague);
    let sender = evm::Address::new([0x39; 20]);
    let recipient = evm::Address::new([0x3a; 20]);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let mut max_u256_bytes = [0u8; 64];
    max_u256_bytes[32..].fill(0xff);
    let max_u256 = U512::from_big_endian(&max_u256_bytes);
    let overflowing_motes = max_u256 / U512::from(DEFAULT_WEI_PER_MOTE) + U512::one();
    seed_evm_balance(&mut tracking_copy, sender, overflowing_motes);

    let result = executor.execute(
        &data_access_layer,
        &mut tracking_copy,
        call_request(sender, Some(recipient), Vec::new(), CasperU256::zero()),
    );

    assert!(matches!(
        result,
        Err(Error::Database(DbError::BalanceOverflow { .. }))
    ));
}

#[test]
fn invalid_wei_per_mote_is_rejected() {
    let executor = EvmExecutor::new(EvmConfig {
        wei_per_mote: 0,
        enabled: true,
        ..EvmConfig::default()
    });
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();

    assert!(matches!(
        executor.execute(
            &data_access_layer,
            &mut tracking_copy,
            call_request(
                evm::Address::ZERO,
                Some(evm::Address::ZERO),
                Vec::new(),
                CasperU256::zero()
            )
        ),
        Err(Error::InvalidWeiPerMote)
    ));
}

#[test]
fn system_call_reports_zero_dust_for_whole_mote_state() {
    let executor = executor(EvmSpec::Prague);
    let target = evm::Address::new([0x3b; 20]);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    seed_evm_balance(&mut tracking_copy, target, U512::one());
    seed_evm_code(&mut tracking_copy, target, vec![opcode::STOP]);

    let outcome = executor
        .execute_system_call(
            &data_access_layer,
            &mut tracking_copy,
            SystemCallRequest {
                block: block(),
                target,
                input: Vec::new(),
            },
        )
        .expect("system call should execute");

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(outcome.dust_motes, U512::zero());
    assert_eq!(read_balance(&mut tracking_copy, target), U512::one());
}

#[test]
fn coinbase_transfer_to_prelinked_beneficiary_credits_proposer_account() {
    let executor = executor(EvmSpec::Prague);
    let sender = evm::Address::new([1; 20]);
    let proposer_secret_key =
        SecretKey::ed25519_from_bytes([42; SecretKey::ED25519_LENGTH]).unwrap();
    let proposer = PublicKey::from(&proposer_secret_key);
    let proposer_account_hash = proposer.to_account_hash();
    let beneficiary = evm::Address::from_block_proposer_public_key(&proposer);
    let proposer_main_purse = URef::new([8; 32], AccessRights::READ_ADD_WRITE);
    let proposer_initial_balance = U512::from(1_000u64);
    let transfer_value = CasperU256::from(250u64);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();

    seed_evm_balance(&mut tracking_copy, sender, U512::from(10_000_000u64));
    seed_account(
        &mut tracking_copy,
        proposer_account_hash,
        proposer_main_purse,
        proposer_initial_balance,
    );
    tracking_copy.write(
        Key::Evm(EvmAddr::Account(beneficiary)),
        StoredValue::CLValue(CLValue::from_t(Key::Account(proposer_account_hash)).unwrap()),
    );
    let contract = deploy_code(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        sender,
        coinbase_transfer_init_code(),
    );
    let mut request = call_request(sender, Some(contract), Vec::new(), transfer_value);
    request.block.beneficiary = beneficiary;

    let outcome = executor
        .execute(&data_access_layer, &mut tracking_copy, request)
        .expect("coinbase transfer should execute");

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(
        read_evm_identity(&mut tracking_copy, beneficiary),
        Some(Key::Account(proposer_account_hash))
    );
    assert_eq!(
        read_account_balance(&mut tracking_copy, proposer_account_hash),
        proposer_initial_balance + U512::from(transfer_value)
    );
}

#[test]
fn coinbase_transfer_without_prelink_uses_evm_native_identity() {
    let executor = executor(EvmSpec::Prague);
    let sender = evm::Address::new([1; 20]);
    let proposer_secret_key =
        SecretKey::ed25519_from_bytes([43; SecretKey::ED25519_LENGTH]).unwrap();
    let proposer = PublicKey::from(&proposer_secret_key);
    let proposer_account_hash = proposer.to_account_hash();
    let beneficiary = evm::Address::from_block_proposer_public_key(&proposer);
    let proposer_main_purse = URef::new([9; 32], AccessRights::READ_ADD_WRITE);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();

    seed_evm_balance(&mut tracking_copy, sender, U512::from(10_000_000u64));
    seed_account(
        &mut tracking_copy,
        proposer_account_hash,
        proposer_main_purse,
        U512::zero(),
    );
    let contract = deploy_code(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        sender,
        coinbase_transfer_init_code(),
    );
    let mut request = call_request(sender, Some(contract), Vec::new(), CasperU256::from(250u64));
    request.block.beneficiary = beneficiary;

    let outcome = executor
        .execute(&data_access_layer, &mut tracking_copy, request)
        .expect("coinbase transfer should execute");

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert!(matches!(
        read_evm_identity(&mut tracking_copy, beneficiary),
        Some(Key::URef(_))
    ));
    assert_eq!(
        read_balance(&mut tracking_copy, beneficiary),
        U512::from(250u64)
    );
    assert_eq!(
        read_account_balance(&mut tracking_copy, proposer_account_hash),
        U512::zero()
    );
}

#[test]
fn reading_coinbase_without_credit_creates_only_evm_native_identity() {
    let executor = executor(EvmSpec::Prague);
    let sender = evm::Address::new([1; 20]);
    let proposer_secret_key =
        SecretKey::ed25519_from_bytes([43; SecretKey::ED25519_LENGTH]).unwrap();
    let proposer = PublicKey::from(&proposer_secret_key);
    let beneficiary = evm::Address::from_block_proposer_public_key(&proposer);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();

    seed_evm_balance(&mut tracking_copy, sender, U512::from(10_000_000u64));
    let contract = deploy_code(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        sender,
        coinbase_observer_init_code(),
    );
    let mut request = call_request(sender, Some(contract), Vec::new(), CasperU256::zero());
    request.block.beneficiary = beneficiary;

    let outcome = executor
        .execute(&data_access_layer, &mut tracking_copy, request)
        .expect("coinbase observer should execute");

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert!(matches!(
        read_evm_identity(&mut tracking_copy, beneficiary),
        Some(Key::URef(_))
    ));
}

#[test]
fn coinbase_transfer_to_linked_beneficiary_with_code_executes_code() {
    let executor = executor(EvmSpec::Prague);
    let sender = evm::Address::new([1; 20]);
    let proposer_secret_key =
        SecretKey::ed25519_from_bytes([44; SecretKey::ED25519_LENGTH]).unwrap();
    let proposer = PublicKey::from(&proposer_secret_key);
    let proposer_account_hash = proposer.to_account_hash();
    let beneficiary = evm::Address::from_block_proposer_public_key(&proposer);
    let proposer_main_purse = URef::new([10; 32], AccessRights::READ_ADD_WRITE);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();

    seed_evm_balance(&mut tracking_copy, sender, U512::from(10_000_000u64));
    seed_account(
        &mut tracking_copy,
        proposer_account_hash,
        proposer_main_purse,
        U512::zero(),
    );
    tracking_copy.write(
        Key::Evm(EvmAddr::Account(beneficiary)),
        StoredValue::CLValue(CLValue::from_t(Key::Account(proposer_account_hash)).unwrap()),
    );
    seed_evm_code(&mut tracking_copy, beneficiary, reverting_runtime());
    let contract = deploy_code(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        sender,
        coinbase_transfer_init_code(),
    );
    let mut request = call_request(sender, Some(contract), Vec::new(), CasperU256::from(250u64));
    request.block.beneficiary = beneficiary;

    let outcome = executor
        .execute(&data_access_layer, &mut tracking_copy, request)
        .expect("coinbase transfer should execute EVM code");

    assert_eq!(outcome.status, ExecutionStatus::Revert);
    assert_eq!(
        read_evm_identity(&mut tracking_copy, beneficiary),
        Some(Key::Account(proposer_account_hash))
    );
    assert_eq!(
        read_account_balance(&mut tracking_copy, proposer_account_hash),
        U512::zero()
    );
}

#[test]
fn coinbase_transfer_keeps_existing_evm_native_beneficiary_identity() {
    let executor = executor(EvmSpec::Prague);
    let sender = evm::Address::new([1; 20]);
    let proposer_secret_key =
        SecretKey::ed25519_from_bytes([46; SecretKey::ED25519_LENGTH]).unwrap();
    let proposer = PublicKey::from(&proposer_secret_key);
    let proposer_account_hash = proposer.to_account_hash();
    let beneficiary = evm::Address::from_block_proposer_public_key(&proposer);
    let proposer_main_purse = URef::new([12; 32], AccessRights::READ_ADD_WRITE);
    let existing_purse = URef::new([13; 32], AccessRights::READ_ADD_WRITE);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();

    seed_evm_balance(&mut tracking_copy, sender, U512::from(10_000_000u64));
    seed_account(
        &mut tracking_copy,
        proposer_account_hash,
        proposer_main_purse,
        U512::zero(),
    );
    tracking_copy.write(
        Key::Evm(EvmAddr::Account(beneficiary)),
        StoredValue::CLValue(CLValue::from_t(Key::URef(existing_purse)).unwrap()),
    );
    tracking_copy.write(
        Key::Evm(EvmAddr::Nonce(beneficiary)),
        StoredValue::CLValue(CLValue::from_t(0u64).unwrap()),
    );
    tracking_copy.write(
        Key::Evm(EvmAddr::CodeHash(beneficiary)),
        StoredValue::CLValue(CLValue::from_t(EMPTY_CODE_HASH).unwrap()),
    );
    let contract = deploy_code(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        sender,
        coinbase_transfer_init_code(),
    );
    let mut request = call_request(sender, Some(contract), Vec::new(), CasperU256::from(250u64));
    request.block.beneficiary = beneficiary;

    let outcome = executor
        .execute(&data_access_layer, &mut tracking_copy, request)
        .expect("coinbase transfer should preserve existing identity");

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(
        read_evm_identity(&mut tracking_copy, beneficiary),
        Some(Key::URef(existing_purse))
    );
    assert_eq!(
        read_balance(&mut tracking_copy, beneficiary),
        U512::from(250u64)
    );
    assert_eq!(
        read_account_balance(&mut tracking_copy, proposer_account_hash),
        U512::zero()
    );
}

#[test]
fn coinbase_transfer_keeps_existing_account_beneficiary_identity() {
    let executor = executor(EvmSpec::Prague);
    let sender = evm::Address::new([1; 20]);
    let proposer_secret_key =
        SecretKey::ed25519_from_bytes([47; SecretKey::ED25519_LENGTH]).unwrap();
    let existing_secret_key =
        SecretKey::ed25519_from_bytes([48; SecretKey::ED25519_LENGTH]).unwrap();
    let proposer = PublicKey::from(&proposer_secret_key);
    let existing_account = PublicKey::from(&existing_secret_key);
    let proposer_account_hash = proposer.to_account_hash();
    let existing_account_hash = existing_account.to_account_hash();
    let beneficiary = evm::Address::from_block_proposer_public_key(&proposer);
    let proposer_main_purse = URef::new([14; 32], AccessRights::READ_ADD_WRITE);
    let existing_main_purse = URef::new([15; 32], AccessRights::READ_ADD_WRITE);
    let existing_initial_balance = U512::from(500u64);
    let transfer_value = CasperU256::from(250u64);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();

    seed_evm_balance(&mut tracking_copy, sender, U512::from(10_000_000u64));
    seed_account(
        &mut tracking_copy,
        proposer_account_hash,
        proposer_main_purse,
        U512::zero(),
    );
    seed_account(
        &mut tracking_copy,
        existing_account_hash,
        existing_main_purse,
        existing_initial_balance,
    );
    tracking_copy.write(
        Key::Evm(EvmAddr::Account(beneficiary)),
        StoredValue::CLValue(CLValue::from_t(Key::Account(existing_account_hash)).unwrap()),
    );
    tracking_copy.write(
        Key::Evm(EvmAddr::Nonce(beneficiary)),
        StoredValue::CLValue(CLValue::from_t(0u64).unwrap()),
    );
    tracking_copy.write(
        Key::Evm(EvmAddr::CodeHash(beneficiary)),
        StoredValue::CLValue(CLValue::from_t(EMPTY_CODE_HASH).unwrap()),
    );
    let contract = deploy_code(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        sender,
        coinbase_transfer_init_code(),
    );
    let mut request = call_request(sender, Some(contract), Vec::new(), transfer_value);
    request.block.beneficiary = beneficiary;

    let outcome = executor
        .execute(&data_access_layer, &mut tracking_copy, request)
        .expect("coinbase transfer should preserve existing account identity");

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(
        read_evm_identity(&mut tracking_copy, beneficiary),
        Some(Key::Account(existing_account_hash))
    );
    assert_eq!(
        read_account_balance(&mut tracking_copy, existing_account_hash),
        existing_initial_balance + U512::from(transfer_value)
    );
    assert_eq!(
        read_account_balance(&mut tracking_copy, proposer_account_hash),
        U512::zero()
    );
}

#[test]
fn nonzero_gas_price_does_not_charge_evm_balances() {
    let executor = executor(EvmSpec::Prague);
    let sender = evm::Address::new([1; 20]);
    let recipient = evm::Address::new([2; 20]);
    let proposer_secret_key =
        SecretKey::ed25519_from_bytes([45; SecretKey::ED25519_LENGTH]).unwrap();
    let proposer = PublicKey::from(&proposer_secret_key);
    let proposer_account_hash = proposer.to_account_hash();
    let beneficiary = evm::Address::from_block_proposer_public_key(&proposer);
    let proposer_main_purse = URef::new([11; 32], AccessRights::READ_ADD_WRITE);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let initial_balance = U512::from(10_000_000u64);
    let transfer_value = CasperU256::from(250u64);

    seed_evm_balance(&mut tracking_copy, sender, initial_balance);
    seed_account(
        &mut tracking_copy,
        proposer_account_hash,
        proposer_main_purse,
        U512::zero(),
    );
    let mut block_context = block();
    block_context.beneficiary = beneficiary;
    let request = ExecuteRequest {
        block: block_context,
        kind: ExecuteKind::Call(CallRequest {
            from: sender,
            to: Some(recipient),
            value: transfer_value * CasperU256::from(DEFAULT_WEI_PER_MOTE),
            input: Vec::new(),
            gas_limit: 100_000,
            gas_price: 2,
            nonce: 0,
            validation: CallValidation::Checked,
        }),
    };

    let outcome = executor
        .execute(&data_access_layer, &mut tracking_copy, request)
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
    assert!(matches!(
        read_evm_identity(&mut tracking_copy, beneficiary),
        Some(Key::URef(_))
    ));
    assert_eq!(read_balance(&mut tracking_copy, beneficiary), U512::zero());
    assert_eq!(
        read_account_balance(&mut tracking_copy, proposer_account_hash),
        U512::zero()
    );
}

#[test]
fn unchecked_call_with_calldata_does_not_underflow_unfunded_sender() {
    let evm_config = EvmConfig {
        enabled: true,
        chain_id: 7,
        spec: EvmSpec::Prague,
        block_gas_limit: 30_000_000,
        base_fee: 1_000_000,
        wei_per_mote: DEFAULT_WEI_PER_MOTE,
    };
    let gas_price = evm_config.base_fee_wei();
    let executor = EvmExecutor::new(evm_config);
    let sender = evm::Address::ZERO;
    let recipient = evm::Address::new([0x41; evm::ADDRESS_LENGTH]);
    let input =
        decode_hex("01ffc9a7d9b67a2600000000000000000000000000000000000000000000000000000000");
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let request = ExecuteRequest {
        block: block(),
        kind: ExecuteKind::Call(CallRequest {
            from: sender,
            to: Some(recipient),
            value: CasperU256::zero(),
            input,
            gas_limit: 30_000_000,
            gas_price,
            nonce: 0,
            validation: CallValidation::UncheckedSimulation,
        }),
    };

    let outcome = executor
        .execute(&data_access_layer, &mut tracking_copy, request)
        .expect("unchecked call should remove simulated fee transfers without underflow");

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert!(outcome.output.is_empty());
}

#[test]
fn erc721_mint_approve_and_transfer() {
    let executor = executor(EvmSpec::Prague);
    let owner = evm::Address::new([1; 20]);
    let recipient = evm::Address::new([2; 20]);
    let approved = evm::Address::new([3; 20]);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let nft = deploy(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        owner,
        "MinimalERC721",
    );

    execute_call(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        owner,
        Some(nft),
        calldata("mint(address,uint256)", &[address_word(owner), word(42)]),
    );
    let initial_owner = execute_call(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        owner,
        Some(nft),
        calldata("ownerOf(uint256)", &[word(42)]),
    );
    assert_eq!(decode_address(&initial_owner.output), owner);

    execute_call(
        &executor,
        &data_access_layer,
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
        &data_access_layer,
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
        &data_access_layer,
        &mut tracking_copy,
        owner,
        Some(nft),
        calldata("ownerOf(uint256)", &[word(42)]),
    );
    assert_eq!(decode_address(&final_owner.output), recipient);
}

#[test]
fn storage_zeroes_are_pruned() {
    let executor = executor(EvmSpec::Prague);
    let from = evm::Address::new([1; 20]);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let contract = deploy(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        from,
        "StorageDelete",
    );

    execute_call(
        &executor,
        &data_access_layer,
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
        &data_access_layer,
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

    let prague_executor = executor(EvmSpec::Prague);
    let (mut prague_tracking_copy, prague_data_access_layer, _prague_tempdir) = tracking_copy();
    let prague_contract = deploy(
        &prague_executor,
        &prague_data_access_layer,
        &mut prague_tracking_copy,
        from,
        "SelfDestruct",
    );
    execute_call(
        &prague_executor,
        &prague_data_access_layer,
        &mut prague_tracking_copy,
        from,
        Some(prague_contract),
        calldata("destroy(address)", &[address_word(beneficiary)]),
    );
    assert!(prague_tracking_copy
        .read(&Key::Evm(EvmAddr::Account(prague_contract)))
        .unwrap()
        .is_some());
}

#[test]
fn signed_transactions_require_configured_chain_id() {
    let executor = executor(EvmSpec::Prague);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let missing_chain_id = legacy_transaction_without_chain_id();
    let request = ExecuteRequest {
        block: block(),
        kind: ExecuteKind::Transaction(Box::new(missing_chain_id)),
    };
    assert!(matches!(
        executor.execute(&data_access_layer, &mut tracking_copy, request),
        Err(Error::MissingChainId)
    ));

    let wrong_chain_executor = EvmExecutor::new(EvmConfig {
        enabled: true,
        chain_id: 8,
        spec: EvmSpec::Prague,
        block_gas_limit: 30_000_000,
        base_fee: 0,
        wei_per_mote: DEFAULT_WEI_PER_MOTE,
    });
    let transaction = legacy_transaction(Some(7));
    let request = ExecuteRequest {
        block: block(),
        kind: ExecuteKind::Transaction(Box::new(transaction)),
    };
    assert!(matches!(
        wrong_chain_executor.execute(&data_access_layer, &mut tracking_copy, request),
        Err(Error::ChainIdMismatch {
            expected: 8,
            actual: 7
        })
    ));
}

#[test]
fn signed_fractional_mote_value_is_rejected() {
    let executor = executor(EvmSpec::Prague);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let transaction = legacy_transaction_with_value(Some(7), U256::from(1));
    let request = ExecuteRequest {
        block: block(),
        kind: ExecuteKind::Transaction(Box::new(transaction)),
    };

    assert!(matches!(
        executor.execute(&data_access_layer, &mut tracking_copy, request),
        Err(Error::Transaction(message))
            if message.contains("is not an exact number of motes")
    ));
}

#[test]
fn signed_transaction_sender_uses_linked_casper_account_identity() {
    let executor = executor(EvmSpec::Prague);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
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
        Key::Evm(EvmAddr::Account(transaction.from())),
        StoredValue::CLValue(CLValue::from_t(Key::Account(account_hash)).unwrap()),
    );

    let request = ExecuteRequest {
        block: block(),
        kind: ExecuteKind::Transaction(Box::new(transaction.clone())),
    };
    let outcome = executor
        .execute(&data_access_layer, &mut tracking_copy, request)
        .expect("EVM execution should succeed");

    assert_eq!(outcome.status, ExecutionStatus::Success);
    assert_eq!(outcome.dust_motes, U512::zero());
    assert_eq!(read_evm_nonce(&mut tracking_copy, transaction.from()), 1);
    assert_eq!(
        read_balance(&mut tracking_copy, transaction.from()),
        initial_balance
    );
    match tracking_copy
        .read(&Key::Evm(EvmAddr::Account(transaction.from())))
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
    let executor = executor(EvmSpec::Prague);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let transaction = legacy_transaction(Some(7));
    let signer = transaction
        .signer()
        .expect("transaction should have a signer");
    let account_hash = signer.to_account_hash();
    let initial_balance = U512::from(1_000_000u64);

    seed_evm_balance(&mut tracking_copy, transaction.from(), initial_balance);

    let request = ExecuteRequest {
        block: block(),
        kind: ExecuteKind::Transaction(Box::new(transaction.clone())),
    };
    let outcome = executor
        .execute(&data_access_layer, &mut tracking_copy, request)
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
        .read(&Key::Evm(EvmAddr::Account(transaction.from())))
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
    let executor = executor(EvmSpec::Prague);
    let from = evm::Address::new([1; 20]);
    let recipient = evm::Address::new([2; 20]);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();

    let mut request = checked_call_request(from, Some(recipient), Vec::new(), CasperU256::zero());
    request.block.base_fee = Some(1);
    assert!(matches!(
        executor.execute(&data_access_layer, &mut tracking_copy, request),
        Err(Error::Revm(_))
    ));
}

#[test]
fn prevrandao_uses_block_context() {
    let executor = executor(EvmSpec::Prague);
    let from = evm::Address::new([1; 20]);
    let (mut tracking_copy, data_access_layer, _tempdir) = tracking_copy();
    let contract = execute_call(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        from,
        None,
        prevrandao_contract_init_code(),
    )
    .created_contract_address
    .expect("deploy should return a contract address");

    let outcome = execute_call(
        &executor,
        &data_access_layer,
        &mut tracking_copy,
        from,
        Some(contract),
        Vec::new(),
    );

    assert_eq!(outcome.output.as_slice(), block().prevrandao.as_ref());
}
