use casper_types::{runtime_args, RuntimeArgs, U512};
use core::convert::TryFrom;

use casper_engine_test_support::{
    internal::DEFAULT_ACCOUNT_PUBLIC_KEY, Code, SessionBuilder, SessionTransferInfo,
    TestContextBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_ACCOUNT_INITIAL_BALANCE,
};

const ARG_AMOUNT: &str = "amount";
const ARG_DESTINATION: &str = "destination";
const DESTINATION_PURSE_ONE: &str = "destination_purse_one";
const DESTINATION_PURSE_TWO: &str = "destination_purse_two";
const TRANSFER_AMOUNT_ONE: &str = "transfer_amount_one";
const TRANSFER_AMOUNT_TWO: &str = "transfer_amount_two";
const TRANSFER_WASM: &str = "transfer_main_purse_to_new_purse.wasm";
const TRANSFER_TO_TWO_PURSES: &str = "transfer_main_purse_to_two_purses.wasm";
const NEW_PURSE_NAME: &str = "test_purse";
const SECOND_PURSE_NAME: &str = "second_purse";
const FIRST_TRANSFER_AMOUNT: u64 = 142;
const SECOND_TRANSFER_AMOUNT: u64 = 250;

#[ignore]
#[test]
fn test_check_transfer_success_with_source_only() {
    let mut test_context = TestContextBuilder::new()
        .with_public_key(
            *DEFAULT_ACCOUNT_PUBLIC_KEY,
            U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE),
        )
        .build();

    // Getting main purse URef to verify transfer
    let source_purse = test_context
        .main_purse_address(*DEFAULT_ACCOUNT_ADDR)
        .expect("main purse address");
    // Target purse doesn't exist yet, so only verifying removal from source
    let maybe_target_purse = None;
    let transfer_amount = U512::try_from(FIRST_TRANSFER_AMOUNT).expect("U512 from u64");
    let source_only_session_transfer_info =
        SessionTransferInfo::new(source_purse, maybe_target_purse, transfer_amount);

    // Doing a transfer from main purse to create new purse and store URef under NEW_PURSE_NAME.
    let session_code = Code::from(TRANSFER_WASM);
    let session_args = runtime_args! {
        ARG_DESTINATION => NEW_PURSE_NAME,
        ARG_AMOUNT => transfer_amount
    };
    let session = SessionBuilder::new(session_code, session_args)
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_check_transfer_success(source_only_session_transfer_info)
        .build();
    test_context.run(session);
}

#[ignore]
#[test]
#[should_panic]
fn test_check_transfer_success_with_source_only_errors() {
    let mut test_context = TestContextBuilder::new()
        .with_public_key(
            *DEFAULT_ACCOUNT_PUBLIC_KEY,
            U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE),
        )
        .build();

    // Getting main purse Uref to verify transfer
    let source_purse = test_context
        .main_purse_address(*DEFAULT_ACCOUNT_ADDR)
        .expect("main purse address");
    let maybe_target_purse = None;
    // Setup mismatch between transfer_amount performed and given to trigger assertion.
    let transfer_amount = U512::try_from(FIRST_TRANSFER_AMOUNT).expect("U512 from u64");
    let wrong_transfer_amount = transfer_amount - U512::try_from(100u64).expect("U512 from 64");
    let source_only_session_transfer_info =
        SessionTransferInfo::new(source_purse, maybe_target_purse, transfer_amount);

    // Doing a transfer from main purse to create new purse and store Uref under NEW_PURSE_NAME.
    let session_code = Code::from(TRANSFER_WASM);
    let session_args = runtime_args! {
        ARG_DESTINATION => NEW_PURSE_NAME,
        ARG_AMOUNT => wrong_transfer_amount
    };
    // Handle expected assertion fail.
    let session = SessionBuilder::new(session_code, session_args)
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_check_transfer_success(source_only_session_transfer_info)
        .build();
    test_context.run(session); // will panic if transfer does not work
}

#[ignore]
#[test]
fn test_check_transfer_success_with_source_and_target() {
    let mut test_context = TestContextBuilder::new()
        .with_public_key(
            *DEFAULT_ACCOUNT_PUBLIC_KEY,
            U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE),
        )
        .build();

    // Getting main purse URef to verify transfer
    let source_purse = test_context
        .main_purse_address(*DEFAULT_ACCOUNT_ADDR)
        .expect("main purse address");

    let maybe_target_purse = None;
    let transfer_amount = U512::try_from(SECOND_TRANSFER_AMOUNT).expect("U512 from u64");
    let source_and_target_session_transfer_info =
        SessionTransferInfo::new(source_purse, maybe_target_purse, transfer_amount);

    // Doing a transfer from main purse to create new purse and store URef under NEW_PURSE_NAME.
    let session_code = Code::from(TRANSFER_WASM);
    let session_args = runtime_args! {
        ARG_DESTINATION => NEW_PURSE_NAME,
        ARG_AMOUNT => transfer_amount
    };
    let session = SessionBuilder::new(session_code, session_args)
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_check_transfer_success(source_and_target_session_transfer_info)
        .build();
    test_context.run(session);

    // retrieve newly created purse URef
    test_context
        .query(*DEFAULT_ACCOUNT_ADDR, &[NEW_PURSE_NAME.to_string()])
        .expect("new purse should exist");
}

#[ignore]
#[test]
#[should_panic]
fn test_check_transfer_success_with_target_error() {
    let mut test_context = TestContextBuilder::new()
        .with_public_key(
            *DEFAULT_ACCOUNT_PUBLIC_KEY,
            U512::from(DEFAULT_ACCOUNT_INITIAL_BALANCE),
        )
        .build();

    // Getting main purse URef to verify transfer
    let source_purse = test_context
        .main_purse_address(*DEFAULT_ACCOUNT_ADDR)
        .expect("main purse address");
    let maybe_target_purse = None;

    // Contract will transfer from main purse twice, into two different purses
    // This call will create the purses, so we can get the URef to destination purses.
    let transfer_one_amount = U512::try_from(FIRST_TRANSFER_AMOUNT).expect("U512 from u64");
    let transfer_two_amount = U512::try_from(SECOND_TRANSFER_AMOUNT).expect("U512 from u64");
    let main_purse_transfer_from_amount = transfer_one_amount + transfer_two_amount;
    let source_only_session_transfer_info = SessionTransferInfo::new(
        source_purse,
        maybe_target_purse,
        main_purse_transfer_from_amount,
    );

    // Will create two purses NEW_PURSE_NAME and SECOND_PURSE_NAME
    let session_code = Code::from(TRANSFER_TO_TWO_PURSES);
    let session_args = runtime_args! {
        DESTINATION_PURSE_ONE => NEW_PURSE_NAME,
        TRANSFER_AMOUNT_ONE => transfer_one_amount,
        DESTINATION_PURSE_TWO => SECOND_PURSE_NAME,
        TRANSFER_AMOUNT_TWO => transfer_two_amount,
    };
    let session = SessionBuilder::new(session_code, session_args)
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_check_transfer_success(source_only_session_transfer_info)
        .build();
    test_context.run(session);

    // get account purse by name via get_account()
    let account = test_context
        .get_account(*DEFAULT_ACCOUNT_ADDR)
        .expect("account");

    let new_purse_address = account
        .named_keys()
        .get(NEW_PURSE_NAME)
        .expect("value")
        .into_uref()
        .expect("uref");

    let maybe_target_purse = Some(new_purse_address); // TODO: Put valid URef here
    let source_and_target_session_transfer_info = SessionTransferInfo::new(
        source_purse,
        maybe_target_purse,
        main_purse_transfer_from_amount,
    );

    // Same transfer as before, but with maybe_target_purse active for validating amount into purse
    // The test for total pulled from main purse should not assert.
    // The test for total into NEW_PURSE_NAME is only part of transfer and should assert.
    let session_code = Code::from(TRANSFER_TO_TWO_PURSES);
    let session_args = runtime_args! {
        DESTINATION_PURSE_ONE => NEW_PURSE_NAME,
        TRANSFER_AMOUNT_ONE => transfer_one_amount,
        DESTINATION_PURSE_TWO => SECOND_PURSE_NAME,
        TRANSFER_AMOUNT_TWO => transfer_two_amount,
    };
    let session = SessionBuilder::new(session_code, session_args)
        .with_address(*DEFAULT_ACCOUNT_ADDR)
        .with_authorization_keys(&[*DEFAULT_ACCOUNT_ADDR])
        .with_check_transfer_success(source_and_target_session_transfer_info)
        .build();
    test_context.run(session); // will panic because maybe_target_purse balance isn't correct
}
