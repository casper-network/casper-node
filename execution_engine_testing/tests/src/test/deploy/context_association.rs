use casper_engine_test_support::{
    DeployItemBuilder, ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR,
    DEFAULT_ACCOUNT_KEY, DEFAULT_PAYMENT, PRODUCTION_RUN_GENESIS_REQUEST,
};

use casper_types::{
    runtime_args,
    system::{AUCTION, HANDLE_PAYMENT, MINT},
};

const SYSTEM_CONTRACT_HASHES_WASM: &str = "system_contract_hashes.wasm";
const ARG_AMOUNT: &str = "amount";

#[ignore]
#[test]
fn should_put_system_contract_hashes_to_account_context() {
    let payment_purse_amount = *DEFAULT_PAYMENT;
    let mut builder = LmdbWasmTestBuilder::default();

    let request = {
        let deploy = DeployItemBuilder::new()
            .with_address(*DEFAULT_ACCOUNT_ADDR)
            .with_session_code(SYSTEM_CONTRACT_HASHES_WASM, runtime_args! {})
            .with_empty_payment_bytes(runtime_args! { ARG_AMOUNT => payment_purse_amount})
            .with_authorization_keys(&[*DEFAULT_ACCOUNT_KEY])
            .with_deploy_hash([1; 32])
            .build();

        ExecuteRequestBuilder::new().push_deploy(deploy).build()
    };

    builder
        .run_genesis(PRODUCTION_RUN_GENESIS_REQUEST.clone())
        .exec(request)
        .expect_success()
        .commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("account should exist");

    let named_keys = account.named_keys();

    assert!(named_keys.contains(MINT), "should contain mint");
    assert!(
        named_keys.contains(HANDLE_PAYMENT),
        "should contain handle payment"
    );
    assert!(named_keys.contains(AUCTION), "should contain auction");

    assert_eq!(
        named_keys
            .get(MINT)
            .unwrap()
            .into_entity_hash_addr()
            .expect("should be a hash"),
        builder.get_mint_contract_hash().value(),
        "mint_contract_hash should match"
    );
    assert_eq!(
        named_keys
            .get(HANDLE_PAYMENT)
            .unwrap()
            .into_entity_hash_addr()
            .expect("should be a hash"),
        builder.get_handle_payment_contract_hash().value(),
        "handle_payment_contract_hash should match"
    );
    assert_eq!(
        named_keys
            .get(AUCTION)
            .unwrap()
            .into_entity_hash_addr()
            .expect("should be a hash"),
        builder.get_auction_contract_hash().value(),
        "auction_contract_hash should match"
    );
}
