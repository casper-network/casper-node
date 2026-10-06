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
    system::protocol_upgrade::ProtocolUpgradeError,
};
use casper_types::{
    evm, execution::TransformKindV2, ByteCode, ByteCodeKind, CLValue, ChainspecRegistry, Digest,
    EraId, EvmAddr, EvmConfig, FeeHandling, GenesisAccount, GenesisConfig, HoldBalanceHandling,
    Key, Motes, ProtocolUpgradeConfig, ProtocolVersion, PublicKey, RewardsHandling, SecretKey,
    StorageCosts, StoredValue, SystemConfig, WasmConfig, U256, U512,
};
use once_cell::sync::Lazy;

static FIXTURE_CONFIG: Lazy<EvmConfig> = Lazy::new(|| {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/local/chainspec.toml.in");
    let table: toml::Table = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    table["evm"].clone().try_into().unwrap()
});

#[derive(Clone, Copy)]
struct TestPreinstall {
    name: &'static str,
    address: evm::Address,
}

impl TestPreinstall {
    fn code(self) -> &'static [u8] {
        &FIXTURE_CONFIG.preinstalls[&self.address]
    }

    fn code_hash(self) -> evm::Hash {
        evm::Hash::new(keccak256(self.code()).0)
    }
}

const MULTICALL3: TestPreinstall = TestPreinstall {
    name: "Multicall3",
    address: evm::Address::new(hex!("0xcA11bde05977b3631167028862bE2a173976CA11")),
};

const CREATE2_DEPLOYER: TestPreinstall = TestPreinstall {
    name: "Arachnid CREATE2 deployer",
    address: evm::Address::new(hex!("0x4e59b44847b379578588920cA78FbF26c0B4956C")),
};

const SAFE_SINGLETON_FACTORY: TestPreinstall = TestPreinstall {
    name: "Safe Singleton Factory",
    address: evm::Address::new(hex!("0x914d7Fec6aaC8cd542e72Bca78B30650d45643d7")),
};

const ERC2470_SINGLETON_FACTORY: TestPreinstall = TestPreinstall {
    name: "ERC-2470 Singleton Factory",
    address: evm::Address::new(hex!("0xce0042B868300000d44A59004Da54A005ffdcf9f")),
};

const PERMIT2: TestPreinstall = TestPreinstall {
    name: "Permit2",
    address: evm::Address::new(hex!("0x000000000022D473030F116dDEE9F6B43aC78BA3")),
};

const SENDER_CREATOR_V08: TestPreinstall = TestPreinstall {
    name: "SenderCreator v0.8",
    address: evm::Address::new(hex!("0x449ED7C3e6Fee6a97311d4b55475DF59C44AdD33")),
};

const ENTRYPOINT_V08: TestPreinstall = TestPreinstall {
    name: "EntryPoint v0.8",
    address: evm::Address::new(hex!("0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108")),
};

const PREINSTALLS: &[TestPreinstall] = &[
    MULTICALL3,
    CREATE2_DEPLOYER,
    SAFE_SINGLETON_FACTORY,
    ERC2470_SINGLETON_FACTORY,
    PERMIT2,
    // These runtimes contain immutable references to each other.
    SENDER_CREATOR_V08,
    ENTRYPOINT_V08,
];

fn evm_config(enabled: bool) -> EvmConfig {
    EvmConfig {
        enabled,
        chain_id: 7,
        base_fee: 5_000,
        ..FIXTURE_CONFIG.clone()
    }
}

fn genesis(enabled: bool, enable_entity: bool) -> (LmdbGlobalState, Digest, impl Send) {
    genesis_with_config(evm_config(enabled), enable_entity)
}

fn genesis_with_config(
    evm: EvmConfig,
    enable_entity: bool,
) -> (LmdbGlobalState, Digest, impl Send) {
    let (state, _, tempdir) = make_temporary_global_state([]);
    let secret = SecretKey::ed25519_from_bytes([1; 32]).unwrap();
    let config = GenesisConfig::new(
        vec![GenesisAccount::Account {
            public_key: PublicKey::from(&secret),
            balance: Motes::new(U512::from(1_000_000_000_000u64)),
            validator: None,
        }],
        evm,
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
    upgrade_request_with_config(
        root,
        current_patch,
        evm_config(enabled),
        enable_entity,
        updates,
    )
}

fn upgrade_request_with_config(
    root: Digest,
    current_patch: u32,
    evm: EvmConfig,
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
        evm,
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
                preinstall.code().to_vec(),
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

fn assert_raw_create2_deployment(factory: TestPreinstall) {
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
fn local_chainspec_pins_the_configured_runtimes() {
    assert_eq!(FIXTURE_CONFIG.preinstalls.len(), PREINSTALLS.len());
    for (contract, length, hash) in [
        (
            MULTICALL3,
            3_808,
            "d5c15df687b16f2ff992fc8d767b4216323184a2bbc6ee2f9c398c318e770891",
        ),
        (
            CREATE2_DEPLOYER,
            69,
            "2fa86add0aed31f33a762c9d88e807c475bd51d0f52bd0955754b2608f7e4989",
        ),
        (
            SAFE_SINGLETON_FACTORY,
            69,
            "2fa86add0aed31f33a762c9d88e807c475bd51d0f52bd0955754b2608f7e4989",
        ),
        (
            ERC2470_SINGLETON_FACTORY,
            308,
            "c4d5542b53a8b779595a20a8ddd60e58a6c49d3c3decc2df83ced1c69c8ca807",
        ),
        (
            PERMIT2,
            9_152,
            "c67d1657868aa5146eaf24fb879fb1fdec3d2d493b3683a61c9c2f4fb2851131",
        ),
        (
            SENDER_CREATOR_V08,
            1_217,
            "c69a1b3a000d570bc86eb096ee63a9014a17951ad616d720882ec61432b00fcf",
        ),
        (
            ENTRYPOINT_V08,
            21_738,
            "44e632a24c6f2600cbd5b5b8b4c2d372359112c8b5774297f5fd0a9e64f11f86",
        ),
    ] {
        assert_eq!(contract.code().len(), length, "{} length", contract.name);
        assert_eq!(
            contract.code_hash().to_hex_string(),
            hash,
            "{} hash",
            contract.name
        );
    }
}

#[test]
fn enabled_evm_with_empty_preinstalls_installs_only_predeploys() {
    for enable_entity in [false, true] {
        let config = EvmConfig {
            enabled: true,
            ..Default::default()
        };
        let (state, root, _tempdir) = genesis_with_config(config.clone(), enable_entity);
        for address in [
            eip4788::BEACON_ROOTS_ADDRESS,
            eip2935::BLOCK_HASH_HISTORY_ADDRESS,
        ] {
            assert!(read(&state, root, code_hash_key(address)).is_some());
        }
        for contract in PREINSTALLS {
            assert!(read(&state, root, code_hash_key(contract.address)).is_none());
        }
        let result = state.protocol_upgrade(upgrade_request_with_config(
            root,
            0,
            config,
            enable_entity,
            BTreeMap::new(),
        ));
        let ProtocolUpgradeResult::Success {
            post_state_hash, ..
        } = result
        else {
            panic!("empty-map upgrade failed: {result:?}");
        };
        for contract in PREINSTALLS {
            assert!(read(&state, post_state_hash, code_hash_key(contract.address)).is_none());
        }
    }
}

#[test]
fn chainspec_can_install_arbitrary_runtime_and_extend_it_on_upgrade() {
    let first = evm::Address::new([0x11; 20]);
    let second = evm::Address::new([0x22; 20]);
    let code = hex!("602a60005260206000f3");
    let code_hash = evm::Hash::new(keccak256(code).0);
    for enable_entity in [false, true] {
        let config = EvmConfig {
            enabled: true,
            chain_id: 7,
            preinstalls: BTreeMap::from([(first, code.to_vec().into())]),
            ..Default::default()
        };
        let (state, root, _tempdir) = genesis_with_config(config.clone(), enable_entity);
        assert_eq!(
            read(&state, root, code_hash_key(first)),
            Some(StoredValue::CLValue(CLValue::from_t(code_hash).unwrap()))
        );
        assert!(read(&state, root, code_hash_key(second)).is_none());
        assert!(read(&state, root, code_hash_key(MULTICALL3.address)).is_none());
        let config = EvmConfig {
            preinstalls: BTreeMap::from([(second, code.to_vec().into())]),
            ..config
        };
        let result = state.protocol_upgrade(upgrade_request_with_config(
            root,
            0,
            config,
            enable_entity,
            BTreeMap::new(),
        ));
        let ProtocolUpgradeResult::Success {
            post_state_hash, ..
        } = result
        else {
            panic!("custom-map upgrade failed: {result:?}");
        };
        for address in [first, second] {
            assert_eq!(
                read(&state, post_state_hash, code_hash_key(address)),
                Some(StoredValue::CLValue(CLValue::from_t(code_hash).unwrap()))
            );
        }
        assert!(read(&state, post_state_hash, code_hash_key(MULTICALL3.address)).is_none());
        let outcomes = call_sequence(
            state,
            post_state_hash,
            &[(first, Vec::new()), (second, Vec::new())],
        );
        for outcome in outcomes {
            assert_eq!(outcome.status, ExecutionStatus::Success);
            assert_eq!(outcome.output, word(42));
        }
    }
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
    assert_eq!(outcomes[0].output, address_word(ENTRYPOINT_V08.address));
    assert_eq!(outcomes[1].status, ExecutionStatus::Revert);
    assert_eq!(
        outcomes[1].output,
        bytes_call("Error(string)", b"AA97 should call from EntryPoint")
    );
}

#[test]
fn preinstalled_entrypoint_uses_its_matching_helper_for_sender_creation() {
    for at_genesis in [false, true] {
        let (state, root, _tempdir) = genesis(at_genesis, false);
        let root = if at_genesis {
            root
        } else {
            let result =
                state.protocol_upgrade(upgrade_request(root, 0, true, false, BTreeMap::new()));
            let ProtocolUpgradeResult::Success {
                post_state_hash, ..
            } = result
            else {
                panic!("upgrade failed: {result:?}");
            };
            post_state_hash
        };
        let salt = [0x42; 32];
        let address = create2_address(ERC2470_SINGLETON_FACTORY.address, salt);
        let mut init_code = ERC2470_SINGLETON_FACTORY.address.as_bytes().to_vec();
        init_code.extend_from_slice(&singleton_factory_input(salt));
        let outcomes = call_sequence(
            state,
            root,
            &[
                (ENTRYPOINT_V08.address, selector("senderCreator()")),
                (
                    ENTRYPOINT_V08.address,
                    bytes_call("getSenderAddress(bytes)", &init_code),
                ),
            ],
        );
        assert_eq!(outcomes[0].status, ExecutionStatus::Success);
        assert_eq!(outcomes[0].output, address_word(SENDER_CREATOR_V08.address));
        // getSenderAddress deliberately reverts with the computed sender after
        // EntryPoint -> SenderCreator -> factory successfully creates it.
        let mut expected = selector("SenderAddressResult(address)");
        expected.extend_from_slice(&address_word(address));
        assert_eq!(outcomes[1].status, ExecutionStatus::Revert);
        assert_eq!(outcomes[1].output, expected);
    }
}

#[test]
fn preinstalled_entrypoint_domain_separator_uses_the_current_chain_id() {
    for chain_id in [1, 7, 31_337] {
        let (state, root, _tempdir) = genesis(true, false);
        let config = EvmConfig {
            chain_id,
            ..evm_config(true)
        };
        let outcomes = call_sequence_with_config(
            state,
            root,
            &[(ENTRYPOINT_V08.address, selector("getDomainSeparatorV4()"))],
            config,
        );
        let mut domain = Vec::new();
        domain.extend_from_slice(
            keccak256("EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)").as_slice(),
        );
        domain.extend_from_slice(keccak256("ERC4337").as_slice());
        domain.extend_from_slice(keccak256("1").as_slice());
        domain.extend_from_slice(&word(chain_id));
        domain.extend_from_slice(&address_word(ENTRYPOINT_V08.address));
        assert_eq!(outcomes[0].status, ExecutionStatus::Success);
        assert_eq!(outcomes[0].output, keccak256(domain).as_slice());
    }
}

#[test]
fn preinstalled_entrypoint_starts_empty_and_executes_bundles_with_transient_guard() {
    let account = evm::Address::new([3; 20]);
    let mut nonce = selector("getNonce(address,uint192)");
    nonce.extend_from_slice(&address_word(account));
    nonce.extend_from_slice(&word(0));
    let mut balance = selector("balanceOf(address)");
    balance.extend_from_slice(&address_word(account));
    let mut bundle = selector(
        "handleOps((address,uint256,bytes,bytes,bytes32,uint256,bytes32,bytes,bytes)[],address)",
    );
    bundle.extend_from_slice(&word(64));
    bundle.extend_from_slice(&address_word(evm::Address::new([2; 20])));
    bundle.extend_from_slice(&word(0));
    let (state, root, _tempdir) = genesis(true, false);
    let outcomes = call_sequence(
        state,
        root,
        &[
            (ENTRYPOINT_V08.address, nonce),
            (ENTRYPOINT_V08.address, balance),
            (ENTRYPOINT_V08.address, bundle.clone()),
            (ENTRYPOINT_V08.address, bundle),
        ],
    );
    for outcome in &outcomes[..2] {
        assert_eq!(outcome.status, ExecutionStatus::Success);
        assert_eq!(outcome.output, word(0));
    }
    for outcome in &outcomes[2..] {
        assert_eq!(outcome.status, ExecutionStatus::Success);
        assert!(outcome.output.is_empty());
        assert_eq!(outcome.logs.len(), 1);
        assert_eq!(outcome.logs[0].address, ENTRYPOINT_V08.address);
        assert_eq!(
            outcome.logs[0].topics,
            vec![evm::Topic::new(keccak256("BeforeExecution()").0)]
        );
    }
}
