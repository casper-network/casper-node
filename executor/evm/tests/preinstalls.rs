use std::{collections::BTreeMap, convert::TryInto};

use alloy_primitives::keccak256;
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
    preinstalls::MULTICALL3,
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

fn assert_multicall3(state: &LmdbGlobalState, root: Digest) {
    assert_eq!(
        read(state, root, code_hash_key(MULTICALL3.address)),
        Some(StoredValue::CLValue(
            CLValue::from_t(MULTICALL3.code_hash()).unwrap()
        ))
    );
    assert_eq!(
        read(state, root, bytecode_key()),
        Some(StoredValue::ByteCode(ByteCode::new(
            ByteCodeKind::EvmPrague,
            MULTICALL3.code.to_vec(),
        )))
    );
}

fn call(state: LmdbGlobalState, root: Digest, input: Vec<u8>) -> ExecutionOutcome {
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
    EvmExecutor::new(evm_config(true))
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
                    to: Some(MULTICALL3.address),
                    value: U256::zero(),
                    input,
                    gas_limit: 1_000_000,
                    gas_price: 0,
                    nonce: 0,
                    validation: CallValidation::UncheckedSimulation,
                }),
            },
        )
        .expect("preinstalled contract execution should succeed")
}

fn selector(signature: &str) -> Vec<u8> {
    keccak256(signature.as_bytes())[..4].to_vec()
}

fn word(value: u64) -> [u8; 32] {
    let mut result = [0; 32];
    result[24..].copy_from_slice(&value.to_be_bytes());
    result
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
        assert_multicall3(&state, after);
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
        assert_multicall3(&state, post_state_hash);
        for transform in effects.transforms() {
            if *transform.key() == code_hash_key(MULTICALL3.address)
                || *transform.key() == bytecode_key()
            {
                assert_eq!(*transform.kind(), TransformKindV2::Identity);
            }
        }

        let outcome = call(state, post_state_hash, selector("getBasefee()"));
        assert_eq!(outcome.status, ExecutionStatus::Success);
        assert_eq!(outcome.output, word(5_000_000_000_000));
    }
}

#[test]
fn disabled_genesis_and_upgrade_skip_preinstalls() {
    for enable_entity in [false, true] {
        let (state, root, _tempdir) = genesis(false, enable_entity);
        assert!(read(&state, root, code_hash_key(MULTICALL3.address)).is_none());
        assert!(read(&state, root, bytecode_key()).is_none());
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
        assert!(read(&state, post_state_hash, code_hash_key(MULTICALL3.address)).is_none());
        assert!(read(&state, post_state_hash, bytecode_key()).is_none());
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
    assert_multicall3(&state, post_state_hash);
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
