use casper_engine_test_support::{
    ExecuteRequestBuilder, LmdbWasmTestBuilder, DEFAULT_ACCOUNT_ADDR, DEFAULT_PROTOCOL_VERSION,
    LOCAL_GENESIS_REQUEST,
};
use casper_storage::{
    data_access_layer::{
        BalanceIdentifier, BalanceIdentifierPurseRequest, BalanceIdentifierPurseResult,
    },
    global_state::state::StateProvider,
};
use casper_types::{runtime_args, Key, StoredValue, URef, U512};

const CONTRACT_CREATE_PURSE: &str = "transfer_main_purse_to_new_purse.wasm";
const CONTRACT_NON_STANDARD_PAYMENT: &str = "non_standard_payment.wasm";
const ARG_AMOUNT: &str = "amount";
const ARG_DESTINATION: &str = "destination";
const ARG_SOURCE_UREF: &str = "source";
const SOURCE_PURSE_NAME: &str = "audit-82-source-purse";

fn handle_payment_payment_purse_balance(builder: &LmdbWasmTestBuilder) -> U512 {
    let state_hash = builder.get_post_state_hash();
    let protocol_version = DEFAULT_PROTOCOL_VERSION;
    let request = BalanceIdentifierPurseRequest::new(
        state_hash,
        protocol_version,
        BalanceIdentifier::Payment,
    );
    let purse_addr = match builder.data_access_layer().balance_purse(request) {
        BalanceIdentifierPurseResult::Success { purse_addr } => purse_addr,
        other => panic!("unexpected balance_purse result: {:?}", other),
    };
    let balance_key = Key::Balance(purse_addr);
    match builder.query(None, balance_key, &[]) {
        Ok(StoredValue::CLValue(cl_value)) => cl_value
            .into_t::<U512>()
            .unwrap_or_else(|err| panic!("failed to read payment purse balance: {:?}", err)),
        Ok(other) => panic!(
            "expected CLValue for payment purse balance, got {:?}",
            other
        ),
        Err(err) => panic!("failed to query payment purse balance: {}", err),
    }
}

fn named_account_purse(builder: &LmdbWasmTestBuilder, name: &str) -> Option<URef> {
    let entity = builder
        .get_entity_with_named_keys_by_account_hash(*DEFAULT_ACCOUNT_ADDR)
        .expect("default account should exist after genesis");
    entity
        .named_keys()
        .get(name)
        .and_then(|key| key.into_uref())
}

/// Regression for audit-confirmed-82: session code must not be able to obtain the system
/// handle-payment payment purse and transfer funds into it. Without the fix the deposit lands
/// and is never refunded, burned, or paid to the proposer - any account can strand funds in the
/// shared payment purse, breaking the payment-purse invariant.
#[ignore]
#[test]
fn session_code_cannot_deposit_into_shared_payment_purse() {
    let mut builder = LmdbWasmTestBuilder::default();
    builder.run_genesis(LOCAL_GENESIS_REQUEST.clone());

    let setup_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_CREATE_PURSE,
        runtime_args! {
            ARG_DESTINATION => SOURCE_PURSE_NAME.to_string(),
            ARG_AMOUNT => U512::from(1_000u64),
        },
    )
    .build();
    builder.exec(setup_request).expect_success().commit();

    let pre_balance = handle_payment_payment_purse_balance(&builder);
    assert!(
        pre_balance.is_zero(),
        "audit-82 regression requires an initially empty payment purse, found {pre_balance}",
    );
    let source =
        named_account_purse(&builder, SOURCE_PURSE_NAME).expect("source purse was not created");

    let exec_request = ExecuteRequestBuilder::standard(
        *DEFAULT_ACCOUNT_ADDR,
        CONTRACT_NON_STANDARD_PAYMENT,
        runtime_args! {
            ARG_AMOUNT => U512::from(100u64),
            ARG_SOURCE_UREF => source,
        },
    )
    .build();
    builder.exec(exec_request).commit();

    // Either the session call must error out (the phase guard rejects `get_payment_purse` outside
    // of payment), or the call succeeds without leaving any motes in the shared payment purse.
    if builder.get_error().is_none() {
        let post_balance = handle_payment_payment_purse_balance(&builder);
        assert!(
            post_balance.is_zero(),
            "session call to get_payment_purse left {post_balance} motes in the shared payment purse",
        );
    }
}
