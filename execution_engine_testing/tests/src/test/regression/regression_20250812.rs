use casper_engine_test_support::{
    ChainspecConfig, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNTS,
    DEFAULT_ACCOUNT_ADDR, DEFAULT_PROTOCOL_VERSION,
};
use casper_types::RuntimeArgs;

const DO_NOTHING_CONTRACT: &str = "do_nothing_stored.wasm";

#[ignore]
#[test]
fn should_correctly_install_and_add_contract_version_with_ae_turned_on() {
    let chainspec = ChainspecConfig::default().with_enable_addressable_entity(true);
    let genesis_request = chainspec
        .create_genesis_request(DEFAULT_ACCOUNTS.to_vec(), DEFAULT_PROTOCOL_VERSION)
        .expect("must create genesis request");

    let mut builder = LmdbWasmTestBuilder::new_temporary_with_config(chainspec);
    builder.run_genesis(genesis_request).commit();

    let install_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        DO_NOTHING_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();

    let install_request_2 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        DO_NOTHING_CONTRACT,
        RuntimeArgs::default(),
    )
    .build();

    builder.exec(install_request_1).expect_success().commit();
    builder.exec(install_request_2).expect_success().commit();
}
