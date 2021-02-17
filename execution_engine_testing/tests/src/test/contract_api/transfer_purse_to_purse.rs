use std::convert::TryFrom;

use casper_types::{runtime_args, system::mint, ApiError, CLValue, Key, RuntimeArgs, U512};

use casper_engine_test_support::{
    internal::{
        ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_PAYMENT,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
};

const CONTRACT_TRANSFER_PURSE_TO_PURSE: &str = "transfer_purse_to_purse.wasm";
const PURSE_TO_PURSE_AMOUNT: u64 = 42;
const ARG_SOURCE: &str = "source";
const ARG_TARGET: &str = "target";
const ARG_AMOUNT: &str = "amount";

#[ignore]
#[test]
fn should_run_purse_to_purse_transfer() {
    let source = "purse:main".to_string();
    let target = "purse:secondary".to_string();

    let exec_request_1 = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_PURSE,
        runtime_args! {
            ARG_SOURCE => source,
            ARG_TARGET => target,
            ARG_AMOUNT => U512::from(PURSE_TO_PURSE_AMOUNT)
        },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();
    builder
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request_1)
        .expect_success()
        .commit()
        .finish();

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get genesis account");

    // Get the `purse_transfer_result` for a given
    let purse_transfer_result_key =
        default_account.named_keys()["purse_transfer_result"].normalize();
    let purse_transfer_result = CLValue::try_from(
        builder
            .query(None, purse_transfer_result_key, &[])
            .expect("should have purse transfer result"),
    )
    .expect("should be a CLValue")
    .into_t::<String>()
    .expect("should be String");
    // Main assertion for the result of `transfer_from_purse_to_purse`
    assert_eq!(
        purse_transfer_result,
        format!("{:?}", Result::<_, ApiError>::Ok(()),)
    );

    let main_purse_balance_key = default_account.named_keys()["main_purse_balance"].normalize();
    let main_purse_balance = CLValue::try_from(
        builder
            .query(None, main_purse_balance_key, &[])
            .expect("should have main purse balance"),
    )
    .expect("should be a CLValue")
    .into_t::<U512>()
    .expect("should be U512");

    // Assert secondary purse value after successful transfer
    let purse_secondary_key = default_account.named_keys()["purse:secondary"];
    let _purse_main_key = default_account.named_keys()["purse:main"];

    // Lookup key used to find the actual purse uref
    // TODO: This should be more consistent
    let purse_secondary_lookup_key = purse_secondary_key
        .as_uref()
        .unwrap()
        .remove_access_rights()
        .to_formatted_string();

    let mint_contract_hash = builder.get_mint_contract_hash();
    let mint_contract = builder
        .get_contract(mint_contract_hash)
        .expect("should have mint contract");

    // Find `purse:secondary`.
    let purse_secondary_uref = mint_contract.named_keys()[&purse_secondary_lookup_key];
    let purse_secondary_key: Key = purse_secondary_uref.normalize();
    let purse_secondary_balance = CLValue::try_from(
        builder
            .query(None, purse_secondary_key, &[])
            .expect("should have main purse balance"),
    )
    .expect("should be a CLValue")
    .into_t::<U512>()
    .expect("should be U512");

    // Final balance of the destination purse
    assert_eq!(purse_secondary_balance, U512::from(PURSE_TO_PURSE_AMOUNT));
    assert_eq!(
        main_purse_balance,
        U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE) - *DEFAULT_PAYMENT - PURSE_TO_PURSE_AMOUNT
    );
}

#[ignore]
#[test]
fn should_run_purse_to_purse_transfer_with_error() {
    // This test runs a contract that's after every call extends the same key with
    // more data
    let source = "purse:main".to_string();
    let target = "purse:secondary".to_string();
    let exec_request_1 = ExecuteRequestBuilder::standard(
       *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_PURSE,
        runtime_args! { ARG_SOURCE => source, ARG_TARGET => target, ARG_AMOUNT => U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE + 1) },
    )
        .build();
    let mut builder = InMemoryWasmTestBuilder::default();
    builder
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request_1)
        .expect_success()
        .commit()
        .finish();

    let default_account = builder
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("should get genesis account");

    // Get the `purse_transfer_result` for a given
    let purse_transfer_result_key =
        default_account.named_keys()["purse_transfer_result"].normalize();
    let purse_transfer_result = CLValue::try_from(
        builder
            .query(None, purse_transfer_result_key, &[])
            .expect("should have purse transfer result"),
    )
    .expect("should be a CLValue")
    .into_t::<String>()
    .expect("should be String");
    // Main assertion for the result of `transfer_from_purse_to_purse`
    assert_eq!(
        purse_transfer_result,
        format!(
            "{:?}",
            Result::<(), ApiError>::Err(mint::Error::InsufficientFunds.into())
        ),
    );

    // Obtain main purse's balance
    let main_purse_balance_key = default_account.named_keys()["main_purse_balance"].normalize();
    let main_purse_balance = CLValue::try_from(
        builder
            .query(None, main_purse_balance_key, &[])
            .expect("should have main purse balance"),
    )
    .expect("should be a CLValue")
    .into_t::<U512>()
    .expect("should be U512");

    // Assert secondary purse value after successful transfer
    let purse_secondary_key = default_account.named_keys()["purse:secondary"];
    let _purse_main_key = default_account.named_keys()["purse:main"];

    // Lookup key used to find the actual purse uref
    // TODO: This should be more consistent
    let purse_secondary_lookup_key = purse_secondary_key
        .as_uref()
        .unwrap()
        .remove_access_rights()
        .to_formatted_string();

    let mint_contract_uref = builder.get_mint_contract_hash();
    let mint_contract = builder
        .get_contract(mint_contract_uref)
        .expect("should have mint contract");

    // Find `purse:secondary` for a balance
    let purse_secondary_uref = mint_contract.named_keys()[&purse_secondary_lookup_key];
    let purse_secondary_key: Key = purse_secondary_uref.normalize();
    let purse_secondary_balance = CLValue::try_from(
        builder
            .query(None, purse_secondary_key, &[])
            .expect("should have main purse balance"),
    )
    .expect("should be a CLValue")
    .into_t::<U512>()
    .expect("should be U512");

    // Final balance of the destination purse equals to 0 as this purse is created
    // as new.
    assert_eq!(purse_secondary_balance, U512::from(0));
    assert_eq!(
        main_purse_balance,
        U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE) - *DEFAULT_PAYMENT
    );
}
