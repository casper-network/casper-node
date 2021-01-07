use num_rational::Ratio;

use casper_engine_test_support::{
    internal::{
        exec_with_return, WasmTestBuilder, DEFAULT_BLOCK_TIME, DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR,
};
use casper_execution_engine::{
    core::engine_state::EngineConfig,
    shared::{stored_value::StoredValue, transform::Transform},
};
use casper_types::{
    contracts::CONTRACT_INITIAL_VERSION, mint::ARG_ROUND_SEIGNIORAGE_RATE, runtime_args,
    ContractHash, ContractPackageHash, ContractVersionKey, DeployHash, ProtocolVersion,
    RuntimeArgs, U512,
};

const DEPLOY_HASH_1: DeployHash = DeployHash::new([1u8; 32]);

#[ignore]
#[test]
fn should_run_mint_install_contract() {
    let mut builder = WasmTestBuilder::default();
    let engine_config = EngineConfig::new();

    builder.run_genesis(&DEFAULT_RUN_GENESIS_REQUEST);

    let round_seigniorage_rate: Ratio<U512> = Ratio::from(U512::one());
    let ((contract_package_hash, mint_hash), ret_urefs, effect): (
        (ContractPackageHash, ContractHash),
        _,
        _,
    ) = exec_with_return::exec(
        engine_config,
        &mut builder,
        *DEFAULT_ACCOUNT_ADDR,
        "mint_install.wasm",
        DEFAULT_BLOCK_TIME,
        DEPLOY_HASH_1,
        "install",
        runtime_args! {
            ARG_ROUND_SEIGNIORAGE_RATE => round_seigniorage_rate,
        },
        vec![],
    )
    .expect("should run successfully");

    // does not return extra urefs
    assert_eq!(ret_urefs.len(), 0);
    assert_ne!(contract_package_hash, mint_hash);

    // should have written a contract under that uref
    let contract_package = match effect.transforms.get(&contract_package_hash.into()) {
        Some(Transform::Write(StoredValue::ContractPackage(contract_package))) => contract_package,

        _ => panic!("Expected contract package to be written under the key"),
    };

    // Checks if the returned package key contains returned mint key.
    assert_eq!(contract_package.versions().len(), 1);
    let mint_version = ContractVersionKey::new(
        ProtocolVersion::V1_0_0.value().major,
        CONTRACT_INITIAL_VERSION,
    );
    assert_eq!(
        contract_package
            .versions()
            .get(&mint_version)
            .cloned()
            .unwrap(),
        mint_hash,
    );

    let contract = match effect.transforms.get(&mint_hash.into()) {
        Some(Transform::Write(StoredValue::Contract(contract))) => contract,

        _ => panic!("Expected contract to be written under the key"),
    };
    assert_eq!(contract.contract_package_hash(), contract_package_hash,);
}
