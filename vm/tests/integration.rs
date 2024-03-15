use borsh::BorshSerialize;
use bytes::Bytes;
use casper_storage::global_state::{
    self,
    state::{lmdb::LmdbGlobalState, CommitProvider, StateProvider},
};
use casper_types::{execution::Effects, Digest, EntityAddr, Key};
use digest::consts::U32;
use vm::{
    backend::{Context, WasmInstance},
    storage::Address,
    ConfigBuilder, VM,
};

// use super::*;
const VM2_TEST_CONTRACT: Bytes = Bytes::from_static(include_bytes!("../vm2-test-contract.wasm"));
const VM2_HARNESS: Bytes = Bytes::from_static(include_bytes!("../vm2-harness.wasm"));
const VM2_CEP18: Bytes = Bytes::from_static(include_bytes!("../vm2-cep18.wasm"));
const VM2_CEP18_CALLER: Bytes = Bytes::from_static(include_bytes!("../vm2-cep18-caller.wasm"));
const VM2_TRAITS: Bytes = Bytes::from_static(include_bytes!("../vm2-trait.wasm"));

#[test]
fn test_contract() {
    let (mut global_state, state_root_hash, _tempdir) =
        global_state::state::lmdb::make_temporary_global_state([]);

    let _effects = run_wasm(
        &mut global_state,
        state_root_hash,
        VM2_TEST_CONTRACT,
        ("Hello, world!".to_string(), 123456789u32),
    );
}

#[test]
fn harness() {
    let (mut global_state, state_root_hash, _tempdir) =
        global_state::state::lmdb::make_temporary_global_state([]);
    run_wasm(&mut global_state, state_root_hash, VM2_HARNESS, ());
}

#[test]
fn cep18() {
    let (mut global_state, mut state_root_hash, _tempdir) =
        global_state::state::lmdb::make_temporary_global_state([]);

    let effects_1 = run_wasm(&mut global_state, state_root_hash, VM2_CEP18, ());

    let contract_hash = {
        let mut values: Vec<_> = effects_1
            .transforms()
            .iter()
            .filter(|t| t.key().is_smart_contract_key())
            .collect();
        assert_eq!(values.len(), 1, "{effects_1:?}");
        let transform = values.remove(0);
        let Key::AddressableEntity(EntityAddr::SmartContract(contract_hash)) = transform.key()
        else {
            panic!("Expected a smart contract key")
        };
        *contract_hash
    };

    state_root_hash = global_state
        .commit(state_root_hash, effects_1)
        .expect("Should commit");
    let _effects_2 = run_wasm(
        &mut global_state,
        state_root_hash,
        VM2_CEP18_CALLER,
        (contract_hash,),
    );
}

#[test]
fn traits() {
    let (mut global_state, root_hash, _tempdir) =
        global_state::state::lmdb::make_temporary_global_state([]);
    run_wasm(&mut global_state, root_hash, VM2_TRAITS, ());
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
}

impl ContractRuntime {
    fn execute(&mut self, execute_request: ExecuteRequest) {
        let ExecuteRequest { wasm_bytes, input } = execute_request;
    }
}

const ALICE: [u8; 32] = [100; 32];
const BOB: [u8; 32] = [101; 32];
const CSPR: u64 = 10u64.pow(9);

fn run_wasm<T: BorshSerialize>(
    global_state: &mut LmdbGlobalState,
    pre_state_hash: Digest,
    module_bytes: Bytes,
    input_data: T,
) -> Effects {
    {
        // "Genesis"
        // storage.update_balance(&ALICE, 10 * CSPR).unwrap();
        // storage.update_balance(&BOB, 10 * CSPR).unwrap();
    }

    let mut vm = VM::new();

    let tracking_copy = global_state
        .tracking_copy(pre_state_hash)
        .expect("Obtaining root hash succeed")
        .expect("Root hash exists");

    let _contract_runtime = ContractRuntime { vm: VM::new() };

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
            .prepare(module_bytes, mock_context, config)
            .expect("should prepare");
        eprintln!("gas_limit={GAS_LIMIT}");
        let (result, gas_summary) = instance.call_export("call");
        eprintln!("{result:?} {gas_summary:?}");
        instance.teardown()
    };

    // for (i, transform) in retrieved_context
    //     .storage
    //     .effects()
    //     .transforms()
    //     .iter()
    //     .enumerate()
    // {
    //     eprintln!("{:?} {:?}", i, transform);
    // }

    retrieved_context.storage.effects()
}
