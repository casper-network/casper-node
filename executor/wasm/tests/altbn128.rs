use std::{env, path::PathBuf, sync::Arc};

use borsh::{BorshDeserialize, BorshSerialize};
use casper_executor_wasm::testing::{
    base_execute_builder, base_install_request_builder, make_address_generator, make_executor,
    make_global_state_with_genesis, read_wasm, run_create_contract, run_wasm_session,
};

use casper_executor_wasm::{chainspec_config, chainspec_config::ChainspecConfig};
use casper_executor_wasm_interface::executor::{ExecuteWithProviderError, ExecutionKind};
use casper_storage::global_state::state::CommitProvider;
use once_cell::sync::Lazy;

/// Symlink to chainspec.
pub static CHAINSPEC_SYMLINK: Lazy<PathBuf> = Lazy::new(|| {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/local/")
        .join(chainspec_config::CHAINSPEC_NAME)
});

#[derive(Clone, Debug, BorshSerialize, BorshDeserialize)]
struct Pair {
    /// G1 point
    ax: [u8; 32],
    /// G1 point
    ay: [u8; 32],
    /// G2 point
    bax: [u8; 32],
    /// G2 point
    bay: [u8; 32],
    /// G1 point
    bbx: [u8; 32],
    /// G1 point
    bby: [u8; 32],
}

impl Pair {
    fn zero() -> Self {
        Self {
            ax: [0; 32],
            ay: [0; 32],
            bax: [0; 32],
            bay: [0; 32],
            bbx: [0; 32],
            bby: [0; 32],
        }
    }
}

#[test]
fn should_run_test_suite() {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);

    let (global_state, state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let install_request = base_install_request_builder(&chainspec_config)
        .with_wasm_bytes(read_wasm("vm2_altbn128.wasm"))
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new".to_string())
        .build()
        .expect("should build");

    let create_result = run_create_contract(
        &mut executor,
        &global_state,
        state_root_hash,
        install_request,
    );

    // Compile above test contract with either feature flags: wasm_add_test, wasm_mul_test,
    // wasm_pairing_test, and uncomment gas line. None of these flags will use host functions.
    // bash -c 'cd smart_contracts/contracts &&
    // RUSTFLAGS="--remap-path-prefix=$HOME/.cargo= --remap-path-prefix=$PWD=/dir" cargo --locked
    // build --verbose --release --package altbn128 --features wasm_add_test'

    eprintln!("gas {:?}", create_result.gas_usage());
}

#[test]
fn should_fail_when_passed_wrong_data_to_pairing() {
    let res = run_pairing_endpoint_test(Vec::<u8>::new());
    let res = extract_result_from_pairing_call(res);
    assert_eq!(res, Err(3)); //Expected "invalid input" error code

    let res = run_pairing_endpoint_test(vec![1_u8, 5, 6, 7]); //Some random bytes, insufficient to build input data
    let res = extract_result_from_pairing_call(res);
    assert_eq!(res, Err(3)); //Expected "invalid input" error code
}

fn extract_result_from_pairing_call(
    res: Result<
        casper_executor_wasm_interface::executor::ExecuteWithProviderResult,
        ExecuteWithProviderError,
    >,
) -> Result<bool, u32> {
    assert!(res.is_ok());
    let binding = res.unwrap();
    let maybe_output = binding.output();
    assert!(maybe_output.is_some());
    let res: Result<bool, u32> = borsh::from_slice(&maybe_output.unwrap()).unwrap();
    res
}

#[test]
fn should_consider_zero_points_paired_and_on_curve() {
    let input: Vec<Pair> = vec![];
    let res = run_pairing_endpoint_test(borsh::to_vec(&input).unwrap());
    let provider_result = res.ok().unwrap();
    let result_bytes = provider_result.output().unwrap();
    let deserialized: Result<bool, u32> = borsh::from_slice(&result_bytes).unwrap();
    assert_eq!(deserialized, Ok(true));

    let input = vec![Pair::zero()];
    let res = run_pairing_endpoint_test(borsh::to_vec(&input).unwrap());
    let provider_result = res.ok().unwrap();
    let result_bytes = provider_result.output().unwrap();
    let deserialized: Result<bool, u32> = borsh::from_slice(&result_bytes).unwrap();
    assert_eq!(deserialized, Ok(true));

    let input = vec![Pair::zero(), Pair::zero(), Pair::zero()];
    let res = run_pairing_endpoint_test(borsh::to_vec(&input).unwrap());
    let provider_result = res.ok().unwrap();
    let result_bytes = provider_result.output().unwrap();
    let deserialized: Result<bool, u32> = borsh::from_slice(&result_bytes).unwrap();
    assert_eq!(deserialized, Ok(true));
}

fn run_pairing_endpoint_test<T: BorshSerialize>(
    input: T,
) -> Result<
    casper_executor_wasm_interface::executor::ExecuteWithProviderResult,
    ExecuteWithProviderError,
> {
    let chainspec_config = ChainspecConfig::from_chainspec_path(&*CHAINSPEC_SYMLINK)
        .expect("must get chainspec config");

    let mut executor = make_executor(&chainspec_config);

    let (global_state, mut state_root_hash, _tempdir) = make_global_state_with_genesis();

    let address_generator = make_address_generator();

    let install_request = base_install_request_builder(&chainspec_config)
        .with_wasm_bytes(read_wasm("vm2_altbn128.wasm"))
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_entry_point("new".to_string())
        .build()
        .expect("should build");

    let create_result = run_create_contract(
        &mut executor,
        &global_state,
        state_root_hash,
        install_request,
    );
    let contract_address = *create_result.smart_contract_addr();
    state_root_hash = global_state
        .commit_effects(state_root_hash, create_result.effects().clone())
        .expect("Should commit");

    let run_method = base_execute_builder(&chainspec_config)
        .with_shared_address_generator(Arc::clone(&address_generator))
        .with_transferred_value(0)
        .with_execution_kind(ExecutionKind::Stored {
            address: contract_address,
            entry_point: "pairing".to_owned(),
        })
        .with_serialized_input(input)
        .expect("expected serialized input to be correct")
        .build()
        .expect("should build");
    run_wasm_session(&mut executor, &global_state, state_root_hash, run_method)
}
