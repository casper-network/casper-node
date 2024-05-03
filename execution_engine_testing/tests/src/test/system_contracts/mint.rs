use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, LOCAL_GENESIS_REQUEST,
};
use casper_types::{runtime_args, ProtocolVersion, URef, U512};

use casper_storage::data_access_layer::BalanceIdentifier;
use tempfile::TempDir;

// const TEST_DELEGATOR_INITIAL_ACCOUNT_BALANCE: u64 = 1_000_000 * 1_000_000_000;

const CONTRACT_BURN: &str = "burn.wasm";
const CONTRACT_TRANSFER_TO_NAMED_PURSE: &str = "transfer_to_named_purse.wasm";

const ARG_AMOUNT: &str = "amount";

const ARG_PURSE_NAME: &str = "purse_name";

#[ignore]
#[test]
fn should_empty_purse_when_burning_above_balance() {
    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new(data_dir.as_ref());
    let source = *DEFAULT_ACCOUNT_ADDR;

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    // let delegator_keys = auction::generate_public_keys(1);
    // let validator_keys = auction::generate_public_keys(1);

    // run_genesis_and_create_initial_accounts(
    //     &mut builder,
    //     &validator_keys,
    //     delegator_keys
    //         .iter()
    //         .map(|public_key| public_key.to_account_hash())
    //         .collect::<Vec<_>>(),
    //     U512::from(TEST_DELEGATOR_INITIAL_ACCOUNT_BALANCE),
    // );

    let initial_supply = builder.total_supply(ProtocolVersion::V2_0_0, None);
    let purse_name = "purse";
    let purse_amount = U512::from(10_000_000_000u64);

    // Create purse and transfer tokens to it
    let exec_request = ExecuteRequestBuilder::standard(
        source,
        CONTRACT_TRANSFER_TO_NAMED_PURSE,
        runtime_args! {
            ARG_PURSE_NAME => purse_name,
            ARG_AMOUNT => purse_amount,
        },
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(source)
        .expect("should have account");

    let purse_uref: URef = account
        .named_keys()
        .get(purse_name)
        .unwrap()
        .into_uref()
        .expect("should be uref");

    assert_eq!(
        builder
            .get_purse_balance_result_with_proofs(
                ProtocolVersion::V2_0_0,
                BalanceIdentifier::Purse(purse_uref)
            )
            .total_balance()
            .cloned()
            .unwrap(),
        purse_amount
    );

    // Burn part of tokens in a purse
    let num_of_tokens_to_burn = U512::from(2_000_000_000u64);
    let num_of_tokens_after_burn = U512::from(8_000_000_000u64);

    let exec_request = ExecuteRequestBuilder::standard(
        source,
        CONTRACT_BURN,
        runtime_args! {
            ARG_PURSE_NAME => purse_name,
            ARG_AMOUNT => num_of_tokens_to_burn,
        },
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    assert_eq!(
        builder
            .get_purse_balance_result_with_proofs(
                ProtocolVersion::V2_0_0,
                BalanceIdentifier::Purse(purse_uref)
            )
            .total_balance()
            .cloned()
            .unwrap(),
        num_of_tokens_after_burn
    );

    // Burn rest of tokens in a purse
    let num_of_tokens_to_burn = U512::from(8_000_000_000u64);
    let num_of_tokens_after_burn = U512::zero();

    let exec_request = ExecuteRequestBuilder::standard(
        source,
        CONTRACT_BURN,
        runtime_args! {
            ARG_PURSE_NAME => purse_name,
            ARG_AMOUNT => num_of_tokens_to_burn,
        },
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    assert_eq!(
        builder
            .get_purse_balance_result_with_proofs(
                ProtocolVersion::V2_0_0,
                BalanceIdentifier::Purse(purse_uref)
            )
            .total_balance()
            .cloned()
            .unwrap(),
        num_of_tokens_after_burn
    );

    let supply_after_burns = builder.total_supply(ProtocolVersion::V2_0_0, None);
    let expected_supply_after_burns = initial_supply - U512::from(10_000_000_000u64);

    assert_eq!(supply_after_burns, expected_supply_after_burns);
}

#[ignore]
#[test]
fn should_not_burn_excess_tokens() {
    let data_dir = TempDir::new().expect("should create temp dir");
    let mut builder = LmdbWasmTestBuilder::new(data_dir.as_ref());
    let source = *DEFAULT_ACCOUNT_ADDR;

    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());
    // let delegator_keys = auction::generate_public_keys(1);
    // let validator_keys = auction::generate_public_keys(1);
    //
    // run_genesis_and_create_initial_accounts(
    //     &mut builder,
    //     &validator_keys,
    //     delegator_keys
    //         .iter()
    //         .map(|public_key| public_key.to_account_hash())
    //         .collect::<Vec<_>>(),
    //     U512::from(TEST_DELEGATOR_INITIAL_ACCOUNT_BALANCE),
    // );

    let initial_supply = builder.total_supply(ProtocolVersion::V2_0_0, None);
    let purse_name = "purse";
    let purse_amount = U512::from(10_000_000_000u64);

    // Create purse and transfer tokens to it
    let exec_request = ExecuteRequestBuilder::standard(
        source,
        CONTRACT_TRANSFER_TO_NAMED_PURSE,
        runtime_args! {
            ARG_PURSE_NAME => purse_name,
            ARG_AMOUNT => purse_amount,
        },
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    let account = builder
        .get_entity_with_named_keys_by_account_hash(source)
        .expect("should have account");

    let purse_uref: URef = account
        .named_keys()
        .get(purse_name)
        .unwrap()
        .into_uref()
        .expect("should be uref");

    assert_eq!(
        builder
            .get_purse_balance_result_with_proofs(
                ProtocolVersion::V2_0_0,
                BalanceIdentifier::Purse(purse_uref)
            )
            .total_balance()
            .cloned()
            .unwrap(),
        purse_amount
    );

    // Try to burn more then in a purse
    let num_of_tokens_to_burn = U512::MAX;
    let num_of_tokens_after_burn = U512::zero();

    let exec_request = ExecuteRequestBuilder::standard(
        source,
        CONTRACT_BURN,
        runtime_args! {
            ARG_PURSE_NAME => purse_name,
            ARG_AMOUNT => num_of_tokens_to_burn,
        },
    )
    .build();

    builder.exec(exec_request).expect_success().commit();

    assert_eq!(
        builder
            .get_purse_balance_result_with_proofs(
                ProtocolVersion::V2_0_0,
                BalanceIdentifier::Purse(purse_uref)
            )
            .total_balance()
            .cloned()
            .unwrap(),
        num_of_tokens_after_burn,
    );

    let supply_after_burns = builder.total_supply(ProtocolVersion::V2_0_0, None);
    let expected_supply_after_burns = initial_supply - U512::from(10_000_000_000u64);

    assert_eq!(supply_after_burns, expected_supply_after_burns);
}
