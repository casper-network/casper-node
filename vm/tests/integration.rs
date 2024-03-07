use blake2::{Blake2b, Digest};
use borsh::BorshSerialize;
use casper_storage::global_state::{
    self,
    state::{lmdb::LmdbGlobalState, StateProvider},
};
use digest::consts::U32;
use rand::prelude::*;
use std::{
    borrow::BorrowMut,
    collections::{BTreeMap, HashMap},
    sync::{Arc, RwLock},
};
// use digest::{self, Digest, FixedOutput, Update, HashMarker, typenum::Unsigned, consts::U32};
use bytes::Bytes;
use vm::{
    backend::{Context, WasmInstance},
    storage::{self, Address, Contract, CreateResult, Entry, Manifest, Package},
    ConfigBuilder, VM,
};

// use super::*;
const VM2_TEST_CONTRACT: &[u8] = include_bytes!("../vm2-test-contract.wasm");
const VM2_HARNESS: &[u8] = include_bytes!("../vm2-harness.wasm");
const VM2_CEP18: &[u8] = include_bytes!("../vm2_cep18.wasm");
const VM2_TRAITS: &[u8] = include_bytes!("../vm2-trait.wasm");

#[test]
fn test_contract() {
    run_wasm(
        VM2_TEST_CONTRACT,
        ("Hello, world!".to_string(), 123456789u32),
    );
}

#[test]
fn harness() {
    run_wasm(VM2_HARNESS, ());
}

#[test]
fn cep18() {
    run_wasm(VM2_CEP18, ());
}

#[test]
fn traits() {
    run_wasm(VM2_TRAITS, ());
}

type Blake2b256 = Blake2b<U32>;

fn blake2b256(updater: impl FnOnce(&mut Blake2b256)) -> Address {
    let mut hasher = Blake2b256::new();
    updater(&mut hasher);
    hasher.finalize().into()
}

fn make_contract_address(package_address: &Address, version: u32) -> Address {
    blake2b256(|hasher| {
        hasher.update(&package_address);
        hasher.update(&version.to_le_bytes());
    })
}

/// VM execute request specifies execution context, the wasm bytes, and other necessary information
/// to execute.
pub struct ExecuteRequest {
    /// Wasm module.
    pub wasm_bytes: Bytes,
    /// Input.
    pub input: Bytes,
}

struct ContractRuntime {
    vm: VM,
    global_state: LmdbGlobalState,
}

impl ContractRuntime {
    fn execute(&mut self, execute_request: ExecuteRequest) {
        let ExecuteRequest { wasm_bytes, input } = execute_request;
    }
}

const ALICE: [u8; 32] = [100; 32];
const BOB: [u8; 32] = [101; 32];
const CSPR: u64 = 10u64.pow(9);

fn run_wasm<T: BorshSerialize>(contract_name: &'static [u8], input_data: T) {
    let bytecode = Bytes::from_static(contract_name);

    // let storage = MockStorage::default();
    let (global_state, root_hash, _tempdir) =
        global_state::state::lmdb::make_temporary_global_state([]);

    {
        // "Genesis"
        // storage.update_balance(&ALICE, 10 * CSPR).unwrap();
        // storage.update_balance(&BOB, 10 * CSPR).unwrap();
    }

    let mut vm = VM::new();

    let tracking_copy = global_state
        .tracking_copy(root_hash)
        .expect("Obtaining root hash succeed")
        .expect("Root hash exists");

    let _contract_runtime = ContractRuntime {
        vm: VM::new(),
        global_state: global_state,
    };

    let input = borsh::to_vec(&input_data).map(Bytes::from).unwrap();

    const GAS_LIMIT: u64 = 1_000_000;
    const MEMORY_LIMIT: u32 = 17;

    // let input = b"This is a very long input data.".to_vec();

    let config = ConfigBuilder::new()
        .with_gas_limit(GAS_LIMIT)
        .with_memory_limit(MEMORY_LIMIT)
        .with_input(input)
        .build();

    let mock_context = Context {
        address: [42; 32],
        storage: tracking_copy,
    };

    let retrieved_context = {
        let mut instance = vm
            .prepare(bytecode, mock_context, config)
            .expect("should prepare");
        eprintln!("gas_limit={GAS_LIMIT}");
        let (result, gas_summary) = instance.call_export("call");
        eprintln!("{result:?} {gas_summary:?}");
        instance.teardown()
    };
}
