use std::{env, path::PathBuf, sync::Arc};

use casper_executor_wasm::testing::{
    base_install_request_builder, make_address_generator, make_executor,
    make_global_state_with_genesis, read_wasm, run_create_contract,
};

use casper_executor_wasm::{chainspec_config, chainspec_config::ChainspecConfig};
use once_cell::sync::Lazy;

/// Symlink to chainspec.
pub static CHAINSPEC_SYMLINK: Lazy<PathBuf> = Lazy::new(|| {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../resources/local/")
        .join(chainspec_config::CHAINSPEC_NAME)
});

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
