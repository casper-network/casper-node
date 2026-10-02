use std::{collections::BTreeMap, convert::TryInto};

use alloy_primitives::{hex, keccak256};
use casper_executor_evm::{
    BlockContext, CallRequest, CallValidation, EvmExecutor, ExecuteKind, ExecuteRequest,
    ExecutionOutcome, ExecutionStatus,
};
use casper_storage::{
    block_store::lmdb::LmdbBlockStore,
    data_access_layer::{
        DataAccessLayer, GenesisRequest, GenesisResult, ProtocolUpgradeRequest,
        ProtocolUpgradeResult,
    },
    eip2935, eip4788,
    global_state::state::{
        lmdb::{make_temporary_global_state, LmdbGlobalState},
        CommitProvider, StateProvider,
    },
    preinstalls::{
        EvmPreinstall, CREATE2_DEPLOYER, ERC2470_SINGLETON_FACTORY, MULTICALL3, PERMIT2,
        PREINSTALLS, SAFE_SINGLETON_FACTORY, SENDER_CREATOR_V08,
    },
    system::protocol_upgrade::ProtocolUpgradeError,
};
use casper_types::{
    evm, execution::TransformKindV2, ByteCode, ByteCodeKind, CLValue, ChainspecRegistry, Digest,
    EraId, EvmAddr, EvmConfig, FeeHandling, GenesisAccount, GenesisConfig, HoldBalanceHandling,
    Key, Motes, ProtocolUpgradeConfig, ProtocolVersion, PublicKey, RewardsHandling, SecretKey,
    StorageCosts, StoredValue, SystemConfig, WasmConfig, U256, U512,
};

fn evm_config(enabled: bool) -> EvmConfig {
    EvmConfig {
        enabled,
        chain_id: 7,
        base_fee: 5_000,
        ..Default::default()
    }
}

fn genesis(enabled: bool, enable_entity: bool) -> (LmdbGlobalState, Digest, impl Send) {
    let (state, _, tempdir) = make_temporary_global_state([]);
    let secret = SecretKey::ed25519_from_bytes([1; 32]).unwrap();
    let config = GenesisConfig::new(
        vec![GenesisAccount::Account {
            public_key: PublicKey::from(&secret),
            balance: Motes::new(U512::from(1_000_000_000_000u64)),
            validator: None,
        }],
        evm_config(enabled),
        WasmConfig::default(),
        SystemConfig::default(),
        10,
        10,
        0,
        Default::default(),
        14,
        0,
        HoldBalanceHandling::Accrued,
        0,
        enable_entity,
        None,
        StorageCosts::default(),
        0,
    );
    let result = state.genesis(GenesisRequest::new(
        Digest::hash("preinstall-test-genesis"),
        ProtocolVersion::V2_0_0,
        config,
        ChainspecRegistry::new_with_genesis(b"chainspec", b"accounts"),
    ));
    let GenesisResult::Success {
        post_state_hash, ..
    } = result
    else {
        panic!("genesis failed: {result:?}");
    };
    (state, post_state_hash, tempdir)
}

fn upgrade_request(
    root: Digest,
    current_patch: u32,
    enabled: bool,
    enable_entity: bool,
    updates: BTreeMap<Key, StoredValue>,
) -> ProtocolUpgradeRequest {
    ProtocolUpgradeRequest::new(ProtocolUpgradeConfig::new(
        root,
        ProtocolVersion::from_parts(2, 0, current_patch),
        ProtocolVersion::from_parts(2, 0, current_patch + 1),
        Some(EraId::new(1)),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        updates,
        ChainspecRegistry::new_with_optional_global_state(b"upgrade", None),
        evm_config(enabled),
        FeeHandling::NoFee,
        0,
        u64::MAX,
        0,
        enable_entity,
        RewardsHandling::Standard,
        None,
    ))
}

fn code_hash_key(address: evm::Address) -> Key {
    Key::Evm(EvmAddr::CodeHash(address))
}

fn bytecode_key() -> Key {
    Key::Evm(EvmAddr::ByteCode(MULTICALL3.code_hash()))
}

fn read(state: &LmdbGlobalState, root: Digest, key: Key) -> Option<StoredValue> {
    state
        .tracking_copy(root)
        .unwrap()
        .unwrap()
        .read(&key)
        .unwrap()
}

fn assert_preinstalls(state: &LmdbGlobalState, root: Digest) {
    for preinstall in PREINSTALLS {
        assert_eq!(
            read(state, root, code_hash_key(preinstall.address)),
            Some(StoredValue::CLValue(
                CLValue::from_t(preinstall.code_hash()).unwrap(),
            )),
            "{} code hash",
            preinstall.name,
        );
        assert_eq!(
            read(
                state,
                root,
                Key::Evm(EvmAddr::ByteCode(preinstall.code_hash()))
            ),
            Some(StoredValue::ByteCode(ByteCode::new(
                ByteCodeKind::EvmPrague,
                preinstall.code.to_vec(),
            ))),
            "{} runtime",
            preinstall.name,
        );
    }
}

fn call(state: LmdbGlobalState, root: Digest, input: Vec<u8>) -> ExecutionOutcome {
    call_contract(state, root, MULTICALL3.address, input)
}

fn call_contract(
    state: LmdbGlobalState,
    root: Digest,
    address: evm::Address,
    input: Vec<u8>,
) -> ExecutionOutcome {
    call_sequence(state, root, &[(address, input)]).remove(0)
}

fn call_sequence(
    state: LmdbGlobalState,
    root: Digest,
    calls: &[(evm::Address, Vec<u8>)],
) -> Vec<ExecutionOutcome> {
    call_sequence_with_config(state, root, calls, evm_config(true))
}

fn call_sequence_with_config(
    state: LmdbGlobalState,
    root: Digest,
    calls: &[(evm::Address, Vec<u8>)],
    config: EvmConfig,
) -> Vec<ExecutionOutcome> {
    let data_access_layer = DataAccessLayer {
        state,
        block_store: LmdbBlockStore::new_temporary(64 * 1024 * 1024).unwrap(),
        max_query_depth: 5,
        enable_addressable_entity: false,
    };
    let mut tracking_copy = data_access_layer
        .state
        .tracking_copy(root)
        .unwrap()
        .unwrap();
    let executor = EvmExecutor::new(config);
    calls
        .iter()
        .enumerate()
        .map(|(index, (address, input))| {
            executor
                .execute(
                    &data_access_layer,
                    &mut tracking_copy,
                    ExecuteRequest {
                        block: BlockContext {
                            number: 42,
                            timestamp: 1_714_000_000,
                            beneficiary: evm::Address::ZERO,
                            gas_limit: None,
                            base_fee: None,
                            prevrandao: evm::Hash::new([0x42; 32]),
                        },
                        kind: ExecuteKind::Call(CallRequest {
                            from: evm::Address::new([2; 20]),
                            to: Some(*address),
                            value: U256::zero(),
                            input: input.clone(),
                            gas_limit: 1_000_000,
                            gas_price: 0,
                            nonce: index as u64,
                            validation: CallValidation::UncheckedSimulation,
                        }),
                    },
                )
                .expect("preinstalled contract execution should succeed")
        })
        .collect()
}

fn selector(signature: &str) -> Vec<u8> {
    keccak256(signature.as_bytes())[..4].to_vec()
}

fn word(value: u64) -> [u8; 32] {
    let mut result = [0; 32];
    result[24..].copy_from_slice(&value.to_be_bytes());
    result
}

fn address_word(address: evm::Address) -> [u8; 32] {
    let mut result = [0; 32];
    result[12..].copy_from_slice(address.as_bytes());
    result
}

fn bytes_call(signature: &str, data: &[u8]) -> Vec<u8> {
    let mut input = selector(signature);
    input.extend_from_slice(&word(32));
    input.extend_from_slice(&word(data.len() as u64));
    input.extend_from_slice(data);
    input.resize(4 + 64 + data.len().div_ceil(32) * 32, 0);
    input
}

// Creation code returning a ten-byte runtime whose calls return the word 42.
const TEST_INIT_CODE: &[u8] = &hex!("600a600c600039600a6000f3602a60005260206000f3");

fn create2_address(factory: evm::Address, salt: [u8; 32]) -> evm::Address {
    let mut input = vec![0xff];
    input.extend_from_slice(factory.as_bytes());
    input.extend_from_slice(&salt);
    input.extend_from_slice(keccak256(TEST_INIT_CODE).as_slice());
    evm::Address::new(keccak256(input)[12..].try_into().unwrap())
}

fn singleton_factory_input(salt: [u8; 32]) -> Vec<u8> {
    let mut input = selector("deploy(bytes,bytes32)");
    input.extend_from_slice(&word(64));
    input.extend_from_slice(&salt);
    input.extend_from_slice(&word(TEST_INIT_CODE.len() as u64));
    input.extend_from_slice(TEST_INIT_CODE);
    input.resize(4 + 128, 0);
    input
}

fn assert_raw_create2_deployment(factory: EvmPreinstall) {
    let salt = [0x42; 32];
    let address = create2_address(factory.address, salt);
    let mut input = salt.to_vec();
    input.extend_from_slice(TEST_INIT_CODE);
    let (state, root, _tempdir) = genesis(true, false);
    let outcomes = call_sequence(
        state,
        root,
        &[
            (factory.address, input.clone()),
            (address, Vec::new()),
            (factory.address, input),
        ],
    );
    assert_eq!(outcomes[0].status, ExecutionStatus::Success);
    assert_eq!(outcomes[0].output, address.as_bytes());
    assert_eq!(outcomes[1].status, ExecutionStatus::Success);
    assert_eq!(outcomes[1].output, word(42));
    assert_eq!(outcomes[2].status, ExecutionStatus::Revert);
}

fn aggregate3_input(calls: &[(&str, bool)]) -> Vec<u8> {
    let mut input = selector("aggregate3((address,bool,bytes)[])");
    input.extend_from_slice(&word(32));
    input.extend_from_slice(&word(calls.len() as u64));
    // Array offsets are relative to the first element-offset word. Every
    // tuple contains a 96-byte header and a 64-byte encoded function selector.
    for index in 0..calls.len() {
        input.extend_from_slice(&word((calls.len() * 32 + index * 160) as u64));
    }
    for (signature, allow_failure) in calls {
        input.extend_from_slice(&[0; 12]);
        input.extend_from_slice(MULTICALL3.address.as_bytes());
        input.extend_from_slice(&word(u64::from(*allow_failure)));
        input.extend_from_slice(&word(96));
        input.extend_from_slice(&word(4));
        input.extend_from_slice(&selector(signature));
        input.extend_from_slice(&[0; 28]);
    }
    input
}

#[test]
fn upgrade_installs_preinstalls_and_repeated_upgrade_preserves_them() {
    for enable_entity in [false, true] {
        let (state, before, _tempdir) = genesis(false, enable_entity);
        assert!(read(&state, before, code_hash_key(MULTICALL3.address)).is_none());

        let result = state.protocol_upgrade(upgrade_request(
            before,
            0,
            true,
            enable_entity,
            BTreeMap::new(),
        ));
        let ProtocolUpgradeResult::Success {
            post_state_hash: after,
            ..
        } = result
        else {
            panic!("upgrade failed: {result:?}");
        };
        assert_preinstalls(&state, after);
        for address in [
            eip4788::BEACON_ROOTS_ADDRESS,
            eip2935::BLOCK_HASH_HISTORY_ADDRESS,
        ] {
            assert!(read(&state, after, code_hash_key(address)).is_some());
        }
        assert!(read(&state, before, code_hash_key(MULTICALL3.address)).is_none());
        assert!(read(&state, after, Key::Evm(EvmAddr::Nonce(MULTICALL3.address))).is_none());
        assert!(read(
            &state,
            after,
            Key::Evm(EvmAddr::Account(MULTICALL3.address))
        )
        .is_none());

        let repeated = state.protocol_upgrade(upgrade_request(
            after,
            1,
            true,
            enable_entity,
            BTreeMap::new(),
        ));
        let ProtocolUpgradeResult::Success {
            post_state_hash,
            effects,
        } = repeated
        else {
            panic!("repeated upgrade failed: {repeated:?}");
        };
        assert_preinstalls(&state, post_state_hash);
        for transform in effects.transforms() {
            if PREINSTALLS.iter().any(|preinstall| {
                *transform.key() == code_hash_key(preinstall.address)
                    || *transform.key() == Key::Evm(EvmAddr::ByteCode(preinstall.code_hash()))
            }) {
                assert_eq!(*transform.kind(), TransformKindV2::Identity);
            }
        }

        let outcome = call(state, post_state_hash, selector("getBasefee()"));
        assert_eq!(outcome.status, ExecutionStatus::Success);
        assert_eq!(outcome.output, word(5_000_000_000_000));
    }
}

#[test]
fn genesis_installs_preinstalls_and_upgrade_preserves_them() {
    for enable_entity in [false, true] {
        let (state, root, _tempdir) = genesis(true, enable_entity);
        assert_preinstalls(&state, root);
        for address in [
            eip4788::BEACON_ROOTS_ADDRESS,
            eip2935::BLOCK_HASH_HISTORY_ADDRESS,
        ] {
            assert!(read(&state, root, code_hash_key(address)).is_some());
        }
        let result = state.protocol_upgrade(upgrade_request(
            root,
            0,
            true,
            enable_entity,
            BTreeMap::new(),
        ));
        let ProtocolUpgradeResult::Success {
            post_state_hash,
            effects,
        } = result
        else {
            panic!("upgrade after genesis failed: {result:?}");
        };
        assert_preinstalls(&state, post_state_hash);
        for transform in effects.transforms() {
            if PREINSTALLS.iter().any(|preinstall| {
                *transform.key() == code_hash_key(preinstall.address)
                    || *transform.key() == Key::Evm(EvmAddr::ByteCode(preinstall.code_hash()))
            }) {
                assert_eq!(*transform.kind(), TransformKindV2::Identity);
            }
        }

        let outcome = call(state, root, selector("getChainId()"));
        assert_eq!(outcome.status, ExecutionStatus::Success);
        assert_eq!(outcome.output, word(7));
    }
}

#[test]
fn disabled_genesis_and_upgrade_skip_preinstalls() {
    for enable_entity in [false, true] {
        let (state, root, _tempdir) = genesis(false, enable_entity);
        for preinstall in PREINSTALLS {
            assert!(read(&state, root, code_hash_key(preinstall.address)).is_none());
            assert!(read(
                &state,
                root,
                Key::Evm(EvmAddr::ByteCode(preinstall.code_hash()))
            )
            .is_none());
        }
        for address in [
            eip4788::BEACON_ROOTS_ADDRESS,
            eip2935::BLOCK_HASH_HISTORY_ADDRESS,
        ] {
            assert!(read(&state, root, code_hash_key(address)).is_none());
        }

        let result = state.protocol_upgrade(upgrade_request(
            root,
            0,
            false,
            enable_entity,
            BTreeMap::new(),
        ));
        let ProtocolUpgradeResult::Success {
            post_state_hash, ..
        } = result
        else {
            panic!("disabled EVM upgrade failed: {result:?}");
        };
        for preinstall in PREINSTALLS {
            assert!(read(&state, post_state_hash, code_hash_key(preinstall.address)).is_none());
            assert!(read(
                &state,
                post_state_hash,
                Key::Evm(EvmAddr::ByteCode(preinstall.code_hash()))
            )
            .is_none());
        }
    }
}

#[test]
fn upgrade_rejects_preinstall_conflicts_after_predeploys() {
    let (state, root, _tempdir) = genesis(false, false);
    let conflicting_code = StoredValue::CLValue(CLValue::from_t(evm::Hash::new([1; 32])).unwrap());
    let mut updates = BTreeMap::new();
    updates.insert(code_hash_key(MULTICALL3.address), conflicting_code.clone());

    let result = state.protocol_upgrade(upgrade_request(root, 0, true, false, updates.clone()));
    assert!(matches!(
        result,
        ProtocolUpgradeResult::Failure(ProtocolUpgradeError::EvmPreinstall(_))
    ));
    assert!(read(&state, root, code_hash_key(MULTICALL3.address)).is_none());
    assert!(read(&state, root, bytecode_key()).is_none());
    assert!(read(
        &state,
        root,
        code_hash_key(eip2935::BLOCK_HASH_HISTORY_ADDRESS)
    )
    .is_none());

    updates.insert(
        code_hash_key(eip2935::BLOCK_HASH_HISTORY_ADDRESS),
        conflicting_code,
    );
    let result = state.protocol_upgrade(upgrade_request(root, 0, true, false, updates));
    assert!(matches!(
        result,
        ProtocolUpgradeResult::Failure(ProtocolUpgradeError::EvmPredeploy(_))
    ));
}

#[test]
fn upgrade_repairs_missing_preinstall_bytecode() {
    let (state, root, _tempdir) = genesis(false, false);
    let root = state
        .commit_values(
            root,
            vec![(
                code_hash_key(MULTICALL3.address),
                StoredValue::CLValue(CLValue::from_t(MULTICALL3.code_hash()).unwrap()),
            )],
            Default::default(),
        )
        .unwrap();
    let result = state.protocol_upgrade(upgrade_request(root, 0, true, false, BTreeMap::new()));
    let ProtocolUpgradeResult::Success {
        post_state_hash, ..
    } = result
    else {
        panic!("repair upgrade failed: {result:?}");
    };
    assert_preinstalls(&state, post_state_hash);
}

#[test]
fn preinstalled_multicall3_aggregates_calls_and_honors_allow_failure() {
    for allow_failure in [true, false] {
        let (state, root, _tempdir) = genesis(false, false);
        let result = state.protocol_upgrade(upgrade_request(root, 0, true, false, BTreeMap::new()));
        let ProtocolUpgradeResult::Success {
            post_state_hash, ..
        } = result
        else {
            panic!("upgrade failed: {result:?}");
        };
        let outcome = call(
            state,
            post_state_hash,
            aggregate3_input(&[
                ("getBlockNumber()", false),
                ("getChainId()", false),
                ("missing()", allow_failure),
            ]),
        );
        if !allow_failure {
            assert_eq!(outcome.status, ExecutionStatus::Revert);
            continue;
        }
        assert_eq!(outcome.status, ExecutionStatus::Success);
        assert_eq!(&outcome.output[..32], &word(32));
        assert_eq!(&outcome.output[32..64], &word(3));
        for (index, expected) in [42, 7].into_iter().enumerate() {
            let offset_word = &outcome.output[64 + index * 32..96 + index * 32];
            let offset = u64::from_be_bytes(offset_word[24..].try_into().unwrap()) as usize;
            let tuple = &outcome.output[64 + offset..];
            assert_eq!(&tuple[..32], &word(1));
            assert_eq!(&tuple[32..64], &word(64));
            assert_eq!(&tuple[64..96], &word(32));
            assert_eq!(&tuple[96..128], &word(expected));
        }
        let offset = u64::from_be_bytes(outcome.output[152..160].try_into().unwrap()) as usize;
        let failed_tuple = &outcome.output[64 + offset..];
        assert_eq!(&failed_tuple[..32], &word(0));
        assert_eq!(&failed_tuple[64..96], &word(0));
    }
}

#[test]
fn preinstalled_create2_deployer_deploys_at_the_deterministic_address() {
    assert_raw_create2_deployment(CREATE2_DEPLOYER);
}

#[test]
fn preinstalled_safe_singleton_factory_deploys_at_the_deterministic_address() {
    assert_raw_create2_deployment(SAFE_SINGLETON_FACTORY);
}

#[test]
fn preinstalled_erc2470_factory_deploys_and_returns_zero_on_collision() {
    let salt = [0x42; 32];
    let address = create2_address(ERC2470_SINGLETON_FACTORY.address, salt);
    let input = singleton_factory_input(salt);
    let (state, root, _tempdir) = genesis(true, false);
    let outcomes = call_sequence(
        state,
        root,
        &[
            (ERC2470_SINGLETON_FACTORY.address, input.clone()),
            (address, Vec::new()),
            (ERC2470_SINGLETON_FACTORY.address, input),
        ],
    );
    assert_eq!(outcomes[0].status, ExecutionStatus::Success);
    assert_eq!(outcomes[0].output, address_word(address));
    assert_eq!(outcomes[1].status, ExecutionStatus::Success);
    assert_eq!(outcomes[1].output, word(42));
    assert_eq!(outcomes[2].status, ExecutionStatus::Success);
    assert_eq!(outcomes[2].output, word(0));
}

fn permit2_domain_separator(chain_id: u64) -> [u8; 32] {
    let mut domain = Vec::new();
    domain.extend_from_slice(
        keccak256("EIP712Domain(string name,uint256 chainId,address verifyingContract)").as_slice(),
    );
    domain.extend_from_slice(keccak256("Permit2").as_slice());
    domain.extend_from_slice(&word(chain_id));
    domain.extend_from_slice(&address_word(PERMIT2.address));
    keccak256(domain).0
}

#[test]
fn preinstalled_permit2_domain_separator_uses_the_current_chain_id() {
    for chain_id in [1, 7, 31_337] {
        let (state, root, _tempdir) = genesis(true, false);
        let config = EvmConfig {
            chain_id,
            ..evm_config(true)
        };
        let outcomes = call_sequence_with_config(
            state,
            root,
            &[(PERMIT2.address, selector("DOMAIN_SEPARATOR()"))],
            config,
        );
        assert_eq!(outcomes[0].status, ExecutionStatus::Success);
        assert_eq!(outcomes[0].output, permit2_domain_separator(chain_id));
    }
}

#[test]
fn preinstalled_permit2_accepts_a_signed_allowance_and_rejects_replay() {
    let owner = evm::Address::new(hex!("7E5F4552091A69125d5DfCb7b8C2659029395Bdf"));
    let token = evm::Address::new([3; 20]);
    let spender = evm::Address::new([4; 20]);
    // EIP-712 PermitSingle signed with the public test private key 1, for chain 7,
    // this Permit2 address, token/spender below, amount 123, expiry/deadline
    // 1_800_000_000, and nonce 0. Generated independently with `cast wallet sign`.
    let signature = hex!("056bc518f3598cd1cf57a812964edec4272b771dc0a99ebf3a15266a55c6e0e838337fe89ad91837b517eee46473376525f0aaa96936228551484bbb98d3efac1b");
    let mut permit =
        selector("permit(address,((address,uint160,uint48,uint48),address,uint256),bytes)");
    permit.extend_from_slice(&address_word(owner));
    permit.extend_from_slice(&address_word(token));
    permit.extend_from_slice(&word(123));
    permit.extend_from_slice(&word(1_800_000_000));
    permit.extend_from_slice(&word(0));
    permit.extend_from_slice(&address_word(spender));
    permit.extend_from_slice(&word(1_800_000_000));
    permit.extend_from_slice(&word(256));
    permit.extend_from_slice(&word(signature.len() as u64));
    permit.extend_from_slice(&signature);
    permit.resize(4 + 256 + 128, 0);

    let mut allowance = selector("allowance(address,address,address)");
    allowance.extend_from_slice(&address_word(owner));
    allowance.extend_from_slice(&address_word(token));
    allowance.extend_from_slice(&address_word(spender));
    let (state, root, _tempdir) = genesis(true, false);
    let outcomes = call_sequence(
        state,
        root,
        &[
            (PERMIT2.address, permit.clone()),
            (PERMIT2.address, allowance),
            (PERMIT2.address, permit),
        ],
    );
    assert_eq!(outcomes[0].status, ExecutionStatus::Success);
    assert_eq!(outcomes[1].status, ExecutionStatus::Success);
    assert_eq!(
        outcomes[1].output,
        [word(123), word(1_800_000_000), word(1)].concat()
    );
    assert_eq!(outcomes[2].status, ExecutionStatus::Revert);
    assert_eq!(outcomes[2].output, selector("InvalidNonce()"));
}

#[test]
fn preinstalled_sender_creator_is_linked_and_rejects_unauthorized_creation() {
    let (state, root, _tempdir) = genesis(true, false);
    let outcomes = call_sequence(
        state,
        root,
        &[
            (SENDER_CREATOR_V08.address, selector("entryPoint()")),
            (
                SENDER_CREATOR_V08.address,
                bytes_call("createSender(bytes)", &[0; 20]),
            ),
        ],
    );
    assert_eq!(outcomes[0].status, ExecutionStatus::Success);
    assert_eq!(
        outcomes[0].output,
        address_word(evm::Address::new(hex!(
            "4337084D9E255Ff0702461CF8895CE9E3b5Ff108"
        )))
    );
    assert_eq!(outcomes[1].status, ExecutionStatus::Revert);
    assert_eq!(
        outcomes[1].output,
        bytes_call("Error(string)", b"AA97 should call from EntryPoint")
    );
}
