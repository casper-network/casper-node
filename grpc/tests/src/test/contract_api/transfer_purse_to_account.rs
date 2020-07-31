use std::convert::TryFrom;

use lazy_static::lazy_static;

use casperlabs_engine_test_support::{
    internal::{
        ExecuteRequestBuilder, InMemoryWasmTestBuilder, DEFAULT_PAYMENT,
        DEFAULT_RUN_GENESIS_REQUEST,
    },
    DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
};
use casperlabs_node::components::contract_runtime::shared::{
    stored_value::StoredValue, transform::Transform,
};
use casperlabs_types::{
    account::AccountHash, runtime_args, ApiError, CLValue, Key, RuntimeArgs, TransferResult,
    TransferredTo, U512,
};

const CONTRACT_TRANSFER_PURSE_TO_ACCOUNT: &str = "transfer_purse_to_account.wasm";
const ACCOUNT_1_ADDR: AccountHash = AccountHash::new([42u8; 32]);
lazy_static! {
    static ref ACCOUNT_1_INITIAL_FUND: U512 = *DEFAULT_PAYMENT + 42;
}

#[ignore]
#[test]
fn should_run_purse_to_account_transfer() {
    let account_1_account_hash = ACCOUNT_1_ADDR;
    let genesis_account_hash = DEFAULT_ACCOUNT_ADDR;
    let exec_request_1 = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
        runtime_args! { "target" => account_1_account_hash, "amount" => *ACCOUNT_1_INITIAL_FUND },
    )
    .build();
    let exec_request_2 = ExecuteRequestBuilder::standard(
        account_1_account_hash,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
        runtime_args! { "target" => genesis_account_hash, "amount" => U512::from(1) },
    )
    .build();
    let mut builder = InMemoryWasmTestBuilder::default();

    //
    // Exec 1 - New account [42; 32] is created
    //

    builder
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request_1)
        .expect_success()
        .commit();

    // Get transforms output for genesis account
    let default_account = builder
        .get_account(DEFAULT_ACCOUNT_ADDR)
        .expect("should get genesis account");

    // Obtain main purse's balance
    let final_balance_key = default_account.named_keys()["final_balance"].normalize();
    let final_balance = CLValue::try_from(
        builder
            .query(None, final_balance_key, &[])
            .expect("should have final balance"),
    )
    .expect("should be a CLValue")
    .into_t::<U512>()
    .expect("should be U512");
    assert_eq!(
        final_balance,
        U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE) - (*DEFAULT_PAYMENT * 2) - 42
    );

    // Get the `transfer_result` for a given account
    let transfer_result_key = default_account.named_keys()["transfer_result"].normalize();
    let transfer_result = CLValue::try_from(
        builder
            .query(None, transfer_result_key, &[])
            .expect("should have transfer result"),
    )
    .expect("should be a CLValue")
    .into_t::<String>()
    .expect("should be String");
    // Main assertion for the result of `transfer_from_purse_to_purse`
    assert_eq!(
        transfer_result,
        format!("{:?}", TransferResult::Ok(TransferredTo::NewAccount))
    );

    // Inspect AddKeys for that new account to find it's purse id
    let new_account = builder
        .get_account(ACCOUNT_1_ADDR)
        .expect("should have new account");

    let new_purse = new_account.main_purse();
    // This is the new lookup key that will be present in AddKeys for a mint
    // contract uref
    let new_purse_key = new_purse.remove_access_rights().to_formatted_string();

    // Obtain transforms for a mint account
    let mint_contract_hash = builder.get_mint_contract_hash();

    let mint_contract = builder
        .get_contract(mint_contract_hash)
        .expect("should have mint contract");

    assert!(mint_contract.named_keys().contains_key(&new_purse_key));

    // Find new account's purse uref
    let new_account_purse_uref = &mint_contract.named_keys()[&new_purse_key];
    let purse_secondary_balance = CLValue::try_from(
        builder
            .query(None, new_account_purse_uref.normalize(), &[])
            .expect("should have final balance"),
    )
    .expect("should be a CLValue")
    .into_t::<U512>()
    .expect("should be U512");
    assert_eq!(purse_secondary_balance, *ACCOUNT_1_INITIAL_FUND);

    // Exec 2 - Transfer from new account back to genesis to verify
    // TransferToExisting

    builder
        .exec(exec_request_2)
        .expect_success()
        .commit()
        .finish();

    let account_1 = builder
        .get_account(ACCOUNT_1_ADDR)
        .expect("should get account 1");

    // Obtain main purse's balance
    let final_balance_key = account_1.named_keys()["final_balance"].normalize();
    let final_balance = CLValue::try_from(
        builder
            .query(None, final_balance_key, &[])
            .expect("should have final balance"),
    )
    .expect("should be a CLValue")
    .into_t::<U512>()
    .expect("should be U512");
    assert_eq!(final_balance, U512::from(41));

    // Get the `transfer_result` for a given account
    let transfer_result_key = account_1.named_keys()["transfer_result"].normalize();
    let transfer_result = CLValue::try_from(
        builder
            .query(None, transfer_result_key, &[])
            .expect("should have transfer result"),
    )
    .expect("should be a CLValue")
    .into_t::<String>()
    .expect("should be String");
    // Main assertion for the result of `transfer_from_purse_to_purse`
    assert_eq!(
        transfer_result,
        format!("{:?}", TransferResult::Ok(TransferredTo::ExistingAccount))
    );

    // Get transforms output for genesis
    let transforms = builder.get_transforms();
    let transform = &transforms[1];
    let genesis_transforms = transform
        .get(&Key::Account(DEFAULT_ACCOUNT_ADDR))
        .expect("Unable to find transforms for a genesis account");

    // Genesis account is unchanged
    assert_eq!(genesis_transforms, &Transform::Identity);

    let genesis_transforms = builder.get_genesis_transforms();

    let balance_uref = genesis_transforms
        .iter()
        .find_map(|(k, t)| match (k, t) {
            (uref @ Key::URef(_), Transform::Write(StoredValue::CLValue(cl_value))) =>
            // 100_000_000_000i64 is the initial balance of genesis
            {
                if cl_value.to_owned().into_t::<U512>().unwrap_or_default()
                    == U512::from(100_000_000_000i64)
                {
                    Some(*uref)
                } else {
                    None
                }
            }
            _ => None,
        })
        .expect("Could not find genesis account balance uref");

    let updated_balance = &transform[&balance_uref.normalize()];
    assert_eq!(updated_balance, &Transform::AddUInt512(U512::from(1)));
}

#[ignore]
#[test]
fn should_fail_when_sending_too_much_from_purse_to_account() {
    let account_1_key = ACCOUNT_1_ADDR;

    let exec_request_1 = ExecuteRequestBuilder::standard(
        DEFAULT_ACCOUNT_ADDR,
        CONTRACT_TRANSFER_PURSE_TO_ACCOUNT,
        runtime_args! { "target" => account_1_key, "amount" => U512::max_value() },
    )
    .build();

    let mut builder = InMemoryWasmTestBuilder::default();

    builder
        .run_genesis(&DEFAULT_RUN_GENESIS_REQUEST)
        .exec(exec_request_1)
        .expect_success()
        .commit()
        .finish();

    // Get transforms output for genesis account
    let default_account = builder
        .get_account(DEFAULT_ACCOUNT_ADDR)
        .expect("should get genesis account");

    // Obtain main purse's balance
    let final_balance_key = default_account.named_keys()["final_balance"].normalize();
    let final_balance = CLValue::try_from(
        builder
            .query(None, final_balance_key, &[])
            .expect("should have final balance"),
    )
    .expect("should be a CLValue")
    .into_t::<U512>()
    .expect("should be U512");
    // When trying to send too much coins the balance is left unchanged
    assert_eq!(
        final_balance,
        U512::from(100_000_000_000u64) - *DEFAULT_PAYMENT,
        "final balance incorrect"
    );

    // Get the `transfer_result` for a given account
    let transfer_result_key = default_account.named_keys()["transfer_result"].normalize();
    let transfer_result = CLValue::try_from(
        builder
            .query(None, transfer_result_key, &[])
            .expect("should have transfer result"),
    )
    .expect("should be a CLValue")
    .into_t::<String>()
    .expect("should be String");

    // Main assertion for the result of `transfer_from_purse_to_purse`
    assert_eq!(
        transfer_result,
        format!("{:?}", Result::<(), _>::Err(ApiError::Transfer)),
        "Transfer Error incorrect"
    );
}
