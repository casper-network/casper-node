use std::path::PathBuf;

use alloy_consensus::{SignableTransaction, TxEnvelope, TxLegacy};
use alloy_eips::eip2718::Encodable2718;
use alloy_primitives::{Address as AlloyAddress, Signature, TxKind, U256};
use casper_executor_evm::{
    BlockContext, BlockHashProvider, BlockHashProviderResult, CallRequest, CallValidation, Error,
    EvmExecutor, ExecuteKind, ExecuteRequest, ExecutionStatus, FeeCharge, EMPTY_CODE_HASH,
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
    evm, BlockHash, CLValue, ChainspecRegistry, Digest, GenesisAccount, GenesisConfig,
    HoldBalanceHandling, Key, Motes, ProtocolVersion, PublicKey, SecretKey, StorageCosts,
    StoredValue, SystemConfig, Timestamp, WasmConfig, U256 as CasperU256, U512,
};
use revm::bytecode::opcode;

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
        fee_charge: FeeCharge::Evm,
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
        fee_charge: FeeCharge::Evm,
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
        Some(StoredValue::Evm(evm::EvmValue::Storage(value))) => Some(value.value()),
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
        Some(StoredValue::Evm(evm::EvmValue::Account(account))) => account.main_purse(),
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
        StoredValue::Evm(evm::EvmValue::Account(evm::Account::new(
            0,
            EMPTY_CODE_HASH,
            main_purse,
        ))),
    );
    tracking_copy.write(
        Key::Balance(main_purse.addr()),
        StoredValue::CLValue(CLValue::from_t(balance).unwrap()),
    );
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
fn selfdestruct_cleanup_follows_selected_fork() {
    let from = evm::Address::new([1; 20]);
    let beneficiary = evm::Address::new([2; 20]);

    let shanghai_executor = executor(evm::EvmSpec::Shanghai);
    let (mut shanghai_tracking_copy, _shanghai_tempdir) = tracking_copy();
    let shanghai_contract = deploy(
        &shanghai_executor,
        &mut shanghai_tracking_copy,
        from,
        "SelfDestruct",
    );
    assert_eq!(
        read_storage(
            &mut shanghai_tracking_copy,
            shanghai_contract,
            CasperU256::zero()
        ),
        Some(storage_word(7))
    );
    execute_call(
        &shanghai_executor,
        &mut shanghai_tracking_copy,
        from,
        Some(shanghai_contract),
        calldata("destroy(address)", &[address_word(beneficiary)]),
    );
    assert_eq!(
        shanghai_tracking_copy
            .read(&Key::Evm(evm::EvmAddr::Account(shanghai_contract)))
            .unwrap(),
        None
    );
    assert_eq!(
        shanghai_tracking_copy
            .read(&Key::Balance(
                evm::deterministic_purse(shanghai_contract).addr()
            ))
            .unwrap(),
        None
    );
    assert_eq!(
        read_storage(
            &mut shanghai_tracking_copy,
            shanghai_contract,
            CasperU256::zero()
        ),
        None
    );

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
        fee_charge: FeeCharge::Evm,
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
        fee_charge: FeeCharge::Evm,
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
fn checked_calls_enforce_transaction_validation() {
    let executor = executor(evm::EvmSpec::Prague);
    let from = evm::Address::new([1; 20]);
    let recipient = evm::Address::new([2; 20]);
    let (mut tracking_copy, _tempdir) = tracking_copy();

    let request = checked_call_request(from, Some(recipient), Vec::new(), CasperU256::from(1));
    assert!(matches!(
        executor.execute(&mut tracking_copy, request),
        Err(Error::Revm(_))
    ));
}
