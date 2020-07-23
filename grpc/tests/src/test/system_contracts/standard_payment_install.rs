use casperlabs_engine_test_support::{
    internal::{
        exec_with_return, WasmTestBuilder, DEFAULT_BLOCK_TIME, DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casperlabs_node::components::contract_runtime::{
    core::engine_state::EngineConfig,
    shared::{stored_value::StoredValue, transform::Transform},
};
use casperlabs_types::{runtime_args, ContractHash, RuntimeArgs};

const DEPLOY_HASH_1: [u8; 32] = [1u8; 32];

#[ignore]
#[test]
fn should_run_standard_payment_install_contract() {
    let mut builder = WasmTestBuilder::default();
    let engine_config =
        EngineConfig::new().with_use_system_contracts(cfg!(feature = "use-system-contracts"));

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let (standard_payment_hash, ret_urefs, effect): (ContractHash, _, _) = exec_with_return::exec(
        engine_config,
        &mut builder,
        DEFAULT_ACCOUNT_ADDR,
        "standard_payment_install.wasm",
        DEFAULT_BLOCK_TIME,
        DEPLOY_HASH_1,
        "install",
        runtime_args! {},
        vec![],
    )
    .expect("should run successfully");

    // should return a uref
    assert_eq!(ret_urefs.len(), 0);

    // should have written a contract under that uref
    match effect.transforms.get(&standard_payment_hash.into()) {
        Some(Transform::Write(StoredValue::Contract(_))) => (),

        _ => panic!("Expected contract to be written under the key"),
    }
}
